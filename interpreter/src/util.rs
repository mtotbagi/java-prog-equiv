use std::{collections::HashMap, fs::File, io::BufReader};

use regex::Regex;
use serde_json::Value;

use crate::{Array, ArrayType, ArrayValue, Heap, HeapValue, StackValue};

pub fn get_class_and_method(arg: &str) -> (String, String) {
    let re = Regex::new(r"(.*)\.(.*):(.*)").unwrap();
    let caps = re.captures(arg).unwrap();

    let classname = caps.get(1).unwrap().as_str().replace('.', "/");
    let methodname = caps.get(2).unwrap().as_str().to_string();
    (classname, methodname)
}

pub fn read_json(classname: &str) -> Value {
    let path = "target/decompiled/".to_string() + classname + ".json";
    eprintln!("path {}", path);
    let file = File::open(path).unwrap();
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).unwrap()
}

pub fn empty_input() -> (Vec<StackValue>, Heap) {
    (vec![], Heap { heap: vec![] })
}

pub fn z3_true() -> z3::ast::Bool {
    z3::ast::Bool::from_bool(true)
}

pub fn z3_false() -> z3::ast::Bool {
    z3::ast::Bool::from_bool(false)
}

pub fn create_string(content: &str, symbolic: bool) -> HeapValue {
    let chars: Vec<ArrayValue> = content
        .encode_utf16()
        .map(|c| {
            let bv = if symbolic {
                z3::ast::BV::fresh_const("char", 16)
            } else {
                z3::ast::BV::from_u64(c as u64, 16)
            };
            ArrayValue::Char(c, bv)
        })
        .collect();

    let mut fields = HashMap::new();
    fields.insert(
        "value".to_string(),
        HeapValue::Array(Array::from_values(ArrayType::Char, chars)),
    );
    HeapValue::Object {
        name: "java/lang/String".into(),
        fields,
    }
}

pub fn abs(val: &StackValue) -> StackValue {
    match val {
        StackValue::Int(i, bv) => {
            StackValue::Int(i.wrapping_abs(), bv.bvsgt(0).ite(&bv.bvneg(), bv))
        }
        StackValue::Long(i, bv) => {
            StackValue::Long(i.wrapping_abs(), bv.bvsgt(0).ite(&bv.bvneg(), bv))
        }
        StackValue::Float(f, sym) => StackValue::Float(f.abs(), sym.unary_abs()),
        StackValue::Double(f, sym) => StackValue::Double(f.abs(), sym.unary_abs()),
        StackValue::Ref(..) => panic!("Cannot do abs on ref!"),
    }
}

/// Converts a signed (two's complement) bitvector to a floating point value
/// of the given sort, rounding to nearest, ties to even (as Java does).
/// The `z3` crate does not expose `Z3_mk_fpa_to_fp_signed`, so it is called directly.
pub fn z3_sbv_to_float(bv: &z3::ast::BV, sort: &z3::Sort) -> z3::ast::Float {
    use z3::ast::Ast;
    let ctx = bv.get_ctx();
    let rm = z3::ast::RoundingMode::round_nearest_ties_to_even();
    unsafe {
        let ast = z3_sys::Z3_mk_fpa_to_fp_signed(
            ctx.get_z3_context(),
            rm.get_z3_ast(),
            bv.get_z3_ast(),
            sort.get_z3_sort(),
        )
        .unwrap();
        z3::ast::Float::wrap(ctx, ast)
    }
}

/// Reinterprets the bits of `bv` (IEEE 754 interchange format) as a float of
/// `sort`, like Java's `Double.longBitsToDouble`; `bv` must have the sort's width.
pub fn z3_ieee_bv_to_float(bv: &z3::ast::BV, sort: &z3::Sort) -> z3::ast::Float {
    use z3::ast::Ast;
    let ctx = bv.get_ctx();
    unsafe {
        let ast =
            z3_sys::Z3_mk_fpa_to_fp_bv(ctx.get_z3_context(), bv.get_z3_ast(), sort.get_z3_sort())
                .unwrap();
        z3::ast::Float::wrap(ctx, ast)
    }
}

/// Java's float/double -> int/long conversion: NaN becomes 0, values are
/// rounded towards zero, and out of range values saturate to MIN/MAX.
pub fn z3_float_to_sbv(f: &z3::ast::Float, size: u32) -> z3::ast::BV {
    use z3::ast::{Ast, BV, Float, RoundingMode};
    let is_double = f.get_sort().float_exponent_size() == Some(11);
    // -2^(size-1) and 2^(size-1) are exactly representable in both float and double
    let bound = 2f64.powi(size as i32 - 1);
    let (lo, hi) = if is_double {
        (Float::from_f64(-bound), Float::from_f64(bound))
    } else {
        (
            Float::from_f32(-bound as f32),
            Float::from_f32(bound as f32),
        )
    };
    let min = BV::from_i64(i64::MIN >> (64 - size), size);
    let max = BV::from_i64(i64::MAX >> (64 - size), size);
    let converted = f.to_sbv_with_rounding_mode(&RoundingMode::round_towards_zero(), size);

    let saturated = f.ge(&hi).ite(&max, &f.le(&lo).ite(&min, &converted));
    f.is_nan().ite(&BV::from_i64(0, size), &saturated)
}

/// Models a pure `java/lang/Math` function `double^n -> double` that Z3 cannot
/// express (transcendentals etc.) as an uninterpreted function named `Math.<name>`.
pub fn math_uf(name: &str, args: &[StackValue], concrete: impl Fn(&[f64]) -> f64) -> StackValue {
    let (vals, syms): (Vec<f64>, Vec<z3::ast::Float>) = args
        .iter()
        .map(|a| match a {
            StackValue::Double(v, s) => (*v, s.clone()),
            other => panic!("Math.{name} expects double arguments, got {other:?}"),
        })
        .unzip();
    let double = z3::Sort::double();
    let domain: Vec<&z3::Sort> = vec![&double; args.len()];
    let decl = z3::FuncDecl::new(format!("Math.{name}"), &domain, &double);
    let sym_args: Vec<&dyn z3::ast::Ast> = syms.iter().map(|s| s as &dyn z3::ast::Ast).collect();
    let sym = decl.apply(&sym_args).as_float().unwrap();
    StackValue::Double(concrete(&vals), sym)
}

/// Exact `Math.getExponent`: unbiased exponent bits of a float/double.
pub fn math_get_exponent(val: &StackValue) -> StackValue {
    match val {
        StackValue::Double(d, sym) => {
            let exp = ((d.to_bits() >> 52) & 0x7ff) as i32 - 1023;
            let bits = sym.to_ieee_bv().extract(62, 52).zero_ext(21);
            StackValue::Int(exp, bits.bvsub(z3::ast::BV::from_i64(1023, 32)))
        }
        StackValue::Float(f, sym) => {
            let exp = ((f.to_bits() >> 23) & 0xff) as i32 - 127;
            let bits = sym.to_ieee_bv().extract(30, 23).zero_ext(24);
            StackValue::Int(exp, bits.bvsub(z3::ast::BV::from_i64(127, 32)))
        }
        other => panic!("Math.getExponent expects float/double, got {other:?}"),
    }
}
