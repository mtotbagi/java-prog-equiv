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
