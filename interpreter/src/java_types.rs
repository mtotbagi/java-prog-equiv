use std::collections::HashMap;

use crate::{z3_false, z3_float_to_sbv, z3_sbv_to_float};
use z3::ast::{BV, Dynamic, Float};
use z3::{Sort, ast::RoundingMode};

use crate::Heap;

#[derive(Clone, Debug, PartialEq)]
pub enum StackValue {
    Int(i32, z3::ast::BV),
    Float(f32, Float),
    // The symbolic bool true iff ref is Some
    Ref(Option<u32>, z3::ast::Bool, z3::ast::BV),
    Long(i64, z3::ast::BV),
    Double(f64, Float),
}

impl StackValue {
    pub fn cast(self, to: &SimpleType) -> StackValue {
        let rne = RoundingMode::round_nearest_ties_to_even();
        match (self, to) {
            (StackValue::Int(i, bv), SimpleType::Byte) => {
                StackValue::Int(i as i8 as i32, bv.extract(7, 0).sign_ext(24))
            }
            (StackValue::Int(i, bv), SimpleType::Short) => {
                StackValue::Int(i as i16 as i32, bv.extract(15, 0).sign_ext(16))
            }
            (StackValue::Int(i, bv), SimpleType::Char) => {
                StackValue::Int(i as u16 as i32, bv.extract(15, 0).zero_ext(16))
            }
            (StackValue::Int(i, bv), SimpleType::Long) => {
                StackValue::Long(i as i64, bv.sign_ext(32))
            }
            (StackValue::Int(i, bv), SimpleType::Float) => {
                StackValue::Float(i as f32, z3_sbv_to_float(&bv, &Sort::float32()))
            }
            (StackValue::Int(i, bv), SimpleType::Double) => {
                StackValue::Double(i as f64, z3_sbv_to_float(&bv, &Sort::double()))
            }
            (StackValue::Long(l, bv), SimpleType::Int) => {
                StackValue::Int(l as i32, bv.extract(31, 0))
            }
            (StackValue::Long(l, bv), SimpleType::Float) => {
                StackValue::Float(l as f32, z3_sbv_to_float(&bv, &Sort::float32()))
            }
            (StackValue::Long(l, bv), SimpleType::Double) => {
                StackValue::Double(l as f64, z3_sbv_to_float(&bv, &Sort::double()))
            }
            (StackValue::Float(f, sym), SimpleType::Int) => {
                StackValue::Int(f as i32, z3_float_to_sbv(&sym, 32))
            }
            (StackValue::Float(f, sym), SimpleType::Long) => {
                StackValue::Long(f as i64, z3_float_to_sbv(&sym, 64))
            }
            (StackValue::Float(f, sym), SimpleType::Double) => StackValue::Double(
                f as f64,
                sym.to_fp_with_rounding_mode(&rne, &Sort::double()),
            ),
            (StackValue::Double(d, sym), SimpleType::Int) => {
                StackValue::Int(d as i32, z3_float_to_sbv(&sym, 32))
            }
            (StackValue::Double(d, sym), SimpleType::Long) => {
                StackValue::Long(d as i64, z3_float_to_sbv(&sym, 64))
            }
            (StackValue::Double(d, sym), SimpleType::Float) => StackValue::Float(
                d as f32,
                sym.to_fp_with_rounding_mode(&rne, &Sort::float32()),
            ),
            (value, to) => panic!("Invalid cast of {:?} to {:?}", value, to),
        }
    }

    pub fn get_type(&self) -> StackType {
        match self {
            StackValue::Int(_, _) => StackType::Int,
            StackValue::Float(_, _) => StackType::Float,
            StackValue::Ref(_, _, _) => StackType::Ref,
            StackValue::Long(_, _) => StackType::Long,
            StackValue::Double(_, _) => StackType::Double,
        }
    }

    /// Long and double take two local variable slots
    pub fn is_wide(&self) -> bool {
        matches!(self, StackValue::Long(..) | StackValue::Double(..))
    }

    pub fn to_array_value(&self, ty: ArrayType) -> ArrayValue {
        match (self, ty) {
            (StackValue::Int(i, bv), ArrayType::Int) => ArrayValue::Int(*i, bv.clone()),
            (StackValue::Int(i, bv), ArrayType::Byte) => {
                ArrayValue::Byte(*i as u8, bv.extract(7, 0))
            }
            (StackValue::Int(i, bv), ArrayType::Char) => {
                ArrayValue::Char(*i as u16, bv.extract(15, 0))
            }
            (StackValue::Int(i, bv), ArrayType::Short) => {
                ArrayValue::Short(*i as i16, bv.extract(15, 0))
            }
            (StackValue::Int(i, bv), ArrayType::Boolean) => {
                ArrayValue::Boolean(*i != 0, bv.extract(1, 0))
            }
            (StackValue::Float(f, ast), ArrayType::Float) => ArrayValue::Float(*f, ast.clone()),
            (StackValue::Double(f, ast), ArrayType::Double) => ArrayValue::Double(*f, ast.clone()),
            (StackValue::Long(l, ast), ArrayType::Long) => ArrayValue::Long(*l, ast.clone()),
            _ => panic!("Cannot convert {:?} to {:?}", self, ty),
        }
    }

    pub fn symbolic_values(&self, heap: &Heap) -> Vec<Dynamic> {
        match self {
            StackValue::Int(_, bv) => vec![Dynamic::from_ast(bv)],
            StackValue::Ref(r, bool, bv) => {
                let mut res = vec![Dynamic::from_ast(bool), Dynamic::from_ast(bv)];
                if let Some(r) = *r {
                    res.append(&mut heap.heap[r as usize].symbolic_values(heap));
                }
                res
            }
            StackValue::Float(_, ast) => vec![Dynamic::from_ast(ast)],
            StackValue::Long(_, bv) => vec![Dynamic::from_ast(bv)],
            StackValue::Double(_, ast) => vec![Dynamic::from_ast(ast)],
        }
    }

    pub fn is_ref(&self) -> bool {
        matches!(self, Self::Ref(_, _, _))
    }

    pub(crate) fn to_heap_value(&self, ty: &SimpleType) -> HeapValue {
        match (self, ty) {
            (StackValue::Int(i, bv), SimpleType::Int) => HeapValue::Int(*i, bv.clone()),
            (StackValue::Int(i, bv), SimpleType::Byte) => {
                HeapValue::Byte(*i as u8, bv.extract(7, 0))
            }
            (StackValue::Int(i, bv), SimpleType::Char) => {
                HeapValue::Char(*i as u16, bv.extract(15, 0))
            }
            (StackValue::Int(i, bv), SimpleType::Short) => {
                HeapValue::Short(*i as i16, bv.extract(15, 0))
            }
            (StackValue::Int(i, bv), SimpleType::Boolean) => {
                HeapValue::Boolean(*i != 0, bv.extract(0, 0))
            }
            (StackValue::Float(f, ast), SimpleType::Float) => HeapValue::Float(*f, ast.clone()),
            (StackValue::Double(f, ast), SimpleType::Double) => HeapValue::Double(*f, ast.clone()),
            (StackValue::Long(f, ast), SimpleType::Long) => HeapValue::Long(*f, ast.clone()),
            (StackValue::Ref(a, b, c), SimpleType::SimpleRef(_)) => {
                HeapValue::Ref(*a, b.clone(), c.clone())
            }
            _ => panic!("Cannot convert {:?} to {:?}", self, ty),
        }
    }

    pub fn min(&self, other: &Self) -> Self {
        assert!(self.get_type() == other.get_type());
        match (self, other) {
            (StackValue::Int(lhs, sym_lhs), StackValue::Int(rhs, sym_rhs)) => StackValue::Int(
                (*lhs).min(*rhs),
                sym_lhs.bvslt(sym_rhs).ite(sym_lhs, sym_rhs),
            ),
            (StackValue::Float(lhs, sym_lhs), StackValue::Float(rhs, sym_rhs)) => {
                StackValue::Float((*lhs).min(*rhs), sym_lhs.lt(sym_rhs).ite(sym_lhs, sym_rhs))
            }
            (StackValue::Long(lhs, sym_lhs), StackValue::Long(rhs, sym_rhs)) => StackValue::Long(
                (*lhs).min(*rhs),
                sym_lhs.bvslt(sym_rhs).ite(sym_lhs, sym_rhs),
            ),
            (StackValue::Double(lhs, sym_lhs), StackValue::Double(rhs, sym_rhs)) => {
                StackValue::Double((*lhs).min(*rhs), sym_lhs.lt(sym_rhs).ite(sym_lhs, sym_rhs))
            }
            _ => panic!("Cannot perform min on these types!"),
        }
    }

    pub fn max(&self, other: &Self) -> Self {
        assert!(self.get_type() == other.get_type());
        match (self, other) {
            (StackValue::Int(lhs, sym_lhs), StackValue::Int(rhs, sym_rhs)) => StackValue::Int(
                (*lhs).max(*rhs),
                sym_lhs.bvsgt(sym_rhs).ite(sym_lhs, sym_rhs),
            ),
            (StackValue::Float(lhs, sym_lhs), StackValue::Float(rhs, sym_rhs)) => {
                StackValue::Float((*lhs).max(*rhs), sym_lhs.gt(sym_rhs).ite(sym_lhs, sym_rhs))
            }
            (StackValue::Long(lhs, sym_lhs), StackValue::Long(rhs, sym_rhs)) => StackValue::Long(
                (*lhs).max(*rhs),
                sym_lhs.bvsgt(sym_rhs).ite(sym_lhs, sym_rhs),
            ),
            (StackValue::Double(lhs, sym_lhs), StackValue::Double(rhs, sym_rhs)) => {
                StackValue::Double((*lhs).max(*rhs), sym_lhs.gt(sym_rhs).ite(sym_lhs, sym_rhs))
            }
            _ => panic!("Cannot perform min on these types!"),
        }
    }

    pub fn are_concrete_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (StackValue::Int(i1, _), StackValue::Int(i2, _)) => *i1 == *i2,
            (StackValue::Long(i1, _), StackValue::Long(i2, _)) => *i1 == *i2,
            (StackValue::Ref(r1, ..), StackValue::Ref(r2, ..)) => match (r1, r2) {
                (None, None) => true,
                (Some(_), Some(_)) => todo!(),
                _ => false,
            },
            (StackValue::Float(f1, _), StackValue::Float(f2, _)) => f1.to_bits() == f2.to_bits(),
            (StackValue::Double(f1, _), StackValue::Double(f2, _)) => f1.to_bits() == f2.to_bits(),
            _ => false,
        }
    }

    pub fn are_symbolic_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (StackValue::Int(_, s1), StackValue::Int(_, s2)) => *s1 == *s2,
            (StackValue::Long(_, s1), StackValue::Long(_, s2)) => *s1 == *s2,
            (StackValue::Ref(_, bs1, _s1), StackValue::Ref(_, bs2, _s2)) => {
                if bs1.as_bool() == Some(false) && bs2.as_bool() == Some(false) {
                    todo!();
                } else {
                    true
                }
            }
            (StackValue::Float(_, s1), StackValue::Float(_, s2)) => s1 == s2,
            (StackValue::Double(_, s1), StackValue::Double(_, s2)) => s1 == s2,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArrayValue {
    Int(i32, z3::ast::BV),
    Float(f32, Float),
    Byte(u8, z3::ast::BV),
    Char(u16, z3::ast::BV),
    Short(i16, z3::ast::BV),
    Boolean(bool, z3::ast::BV),
    Long(i64, z3::ast::BV),
    Double(f64, Float),
}

impl ArrayValue {
    pub fn to_stack_value(&self) -> StackValue {
        match self {
            ArrayValue::Int(i, bv) => StackValue::Int(*i, bv.clone()),
            ArrayValue::Float(f, ast) => StackValue::Float(*f, ast.clone()),
            ArrayValue::Byte(i, bv) => StackValue::Int(*i as i32, bv.zero_ext(24)),
            ArrayValue::Char(i, bv) => StackValue::Int(*i as i32, bv.zero_ext(16)),
            ArrayValue::Short(i, bv) => StackValue::Int(*i as i32, bv.sign_ext(16)),
            ArrayValue::Boolean(i, bv) => StackValue::Int(*i as i32, bv.zero_ext(31)),
            ArrayValue::Long(i, bv) => StackValue::Long(*i, bv.clone()),
            ArrayValue::Double(f, ast) => StackValue::Double(*f, ast.clone()),
        }
    }

    pub fn z3(&self) -> Dynamic {
        match self {
            ArrayValue::Int(_, bv)
            | ArrayValue::Byte(_, bv)
            | ArrayValue::Char(_, bv)
            | ArrayValue::Short(_, bv)
            | ArrayValue::Boolean(_, bv)
            | ArrayValue::Long(_, bv) => Dynamic::from_ast(bv),
            ArrayValue::Float(_, ast) | ArrayValue::Double(_, ast) => Dynamic::from_ast(ast),
        }
    }

    pub fn swap_sym_val(&self, sym_val: Dynamic) -> ArrayValue {
        match self {
            ArrayValue::Int(i, _) => ArrayValue::Int(*i, sym_val.as_bv().unwrap()),
            ArrayValue::Float(f, _) => ArrayValue::Float(*f, sym_val.as_float().unwrap()),
            ArrayValue::Byte(b, _) => ArrayValue::Byte(*b, sym_val.as_bv().unwrap()),
            ArrayValue::Char(c, _) => ArrayValue::Char(*c, sym_val.as_bv().unwrap()),
            ArrayValue::Short(s, _) => ArrayValue::Short(*s, sym_val.as_bv().unwrap()),
            ArrayValue::Boolean(b, _) => ArrayValue::Boolean(*b, sym_val.as_bv().unwrap()),
            ArrayValue::Long(d, _) => ArrayValue::Long(*d, sym_val.as_bv().unwrap()),
            ArrayValue::Double(d, _) => ArrayValue::Double(*d, sym_val.as_float().unwrap()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HeapValue {
    Int(i32, z3::ast::BV),
    Long(i64, z3::ast::BV),
    Ref(Option<u32>, z3::ast::Bool, z3::ast::BV),
    Float(f32, Float),
    Double(f64, Float),
    Byte(u8, z3::ast::BV),
    Char(u16, z3::ast::BV),
    Short(i16, z3::ast::BV),
    Boolean(bool, z3::ast::BV),
    Array(Array),
    Object {
        name: String,
        fields: HashMap<String, HeapValue>,
    },
}

impl HeapValue {
    pub fn to_stack_value(&self) -> StackValue {
        match self {
            HeapValue::Int(i, bv) => StackValue::Int(*i, bv.clone()),
            HeapValue::Float(f, ast) => StackValue::Float(*f, ast.clone()),
            HeapValue::Byte(b, bv) => StackValue::Int(*b as i32, bv.zero_ext(24)),
            HeapValue::Short(s, bv) => StackValue::Int(*s as i32, bv.sign_ext(16)),
            HeapValue::Char(c, bv) => StackValue::Int(*c as i32, bv.zero_ext(16)),
            HeapValue::Ref(r, sym_is_null, sym_bv) => {
                StackValue::Ref(*r, sym_is_null.clone(), sym_bv.clone())
            }
            HeapValue::Long(l, bv) => StackValue::Long(*l, bv.clone()),
            HeapValue::Double(f, ast) => StackValue::Double(*f, ast.clone()),
            HeapValue::Boolean(b, bv) => StackValue::Int(*b as i32, bv.zero_ext(31)),
            _ => panic!("Can't convert to StackValue"),
        }
    }

    pub fn symbolic_values(&self, heap: &Heap) -> Vec<z3::ast::Dynamic> {
        // Important: deterministic order!!!
        match self {
            HeapValue::Int(_, bv) => vec![Dynamic::from_ast(bv)],
            HeapValue::Ref(r, bool, bv) => {
                let mut res = vec![Dynamic::from_ast(bool), Dynamic::from_ast(bv)];
                if let Some(r) = *r {
                    res.append(&mut heap.heap[r as usize].symbolic_values(heap));
                }
                res
            }
            HeapValue::Float(_, ast) => vec![Dynamic::from_ast(ast)],
            HeapValue::Byte(_, bv) => vec![Dynamic::from_ast(bv)],
            HeapValue::Char(_, bv) => vec![Dynamic::from_ast(bv)],
            HeapValue::Short(_, bv) => vec![Dynamic::from_ast(bv)],
            HeapValue::Boolean(_, bv) => vec![Dynamic::from_ast(bv)],
            HeapValue::Array(array) => vec![
                Dynamic::from_ast(&array.symbolic),
                Dynamic::from_ast(&array.len),
            ],
            HeapValue::Object { name: _, fields } => {
                let mut keys: Vec<_> = fields.keys().collect();
                keys.sort();
                let mut res = vec![];
                for key in keys {
                    res.append(&mut fields[key].symbolic_values(heap));
                }
                res
            }
            HeapValue::Long(_, bv) => vec![Dynamic::from_ast(bv)],
            HeapValue::Double(_, ast) => vec![Dynamic::from_ast(ast)],
        }
    }

    pub fn default_val(ty: &SimpleType) -> Self {
        match ty {
            SimpleType::Int => HeapValue::Int(0, BV::from_i64(0, 32)),
            SimpleType::Long => HeapValue::Long(0, BV::from_i64(0, 64)),
            SimpleType::Float => HeapValue::Float(0.0, Float::from_f32(0.0)),
            SimpleType::Double => HeapValue::Double(0.0, Float::from_f64(0.0)),
            SimpleType::Byte => HeapValue::Byte(0, BV::from_i64(0, 8)),
            SimpleType::Char => HeapValue::Char(0, BV::from_i64(0, 16)),
            SimpleType::Short => HeapValue::Short(0, BV::from_i64(0, 16)),
            SimpleType::Boolean => HeapValue::Boolean(false, BV::from_i64(0, 1)),
            SimpleType::SimpleRef(..) => HeapValue::Ref(None, z3_false(), BV::from_i64(0, 32)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Array {
    pub ty: ArrayType,
    pub values: Vec<ArrayValue>,
    pub symbolic: z3::ast::Array,
    pub len: z3::ast::BV,
}

impl Array {
    pub fn from_values(ty: ArrayType, values: Vec<ArrayValue>) -> Self {
        let mut symbolic =
            z3::ast::Array::fresh_const("array", &z3::Sort::bitvector(32), &ty.z3_sort());
        for (i, v) in values.iter().enumerate() {
            symbolic = symbolic.store(&z3::ast::BV::from_u64(i as u64, 32), &v.z3());
        }
        let len = z3::ast::BV::from_u64(values.len() as u64, 32);
        Array {
            ty,
            values,
            symbolic,
            len,
        }
    }

    pub fn new_zero(ty: ArrayType, len: StackValue) -> Self {
        let StackValue::Int(x, bv) = len else {
            panic!("Invalid array length!")
        };
        let len = x as usize;
        let default_val = match ty {
            ArrayType::Int => ArrayValue::Int(0, z3::ast::BV::from_i64(0, 32)),
            ArrayType::Float => ArrayValue::Float(0.0, Float::from_f32(0.0)),
            ArrayType::Byte => ArrayValue::Byte(0, z3::ast::BV::from_i64(0, 8)),
            ArrayType::Char => ArrayValue::Char(0, z3::ast::BV::from_i64(0, 16)),
            ArrayType::Short => ArrayValue::Short(0, z3::ast::BV::from_i64(0, 16)),
            ArrayType::Boolean => ArrayValue::Boolean(false, z3::ast::BV::from_i64(0, 1)),
            ArrayType::Long => ArrayValue::Long(0, z3::ast::BV::from_i64(0, 64)),
            ArrayType::Double => ArrayValue::Double(0.0, Float::from_f64(0.0)),
        };
        // 0 as default
        let arr = vec![default_val.clone(); len];

        Self {
            ty,
            values: arr,
            symbolic: z3::ast::Array::const_array(&z3::Sort::bitvector(32), &default_val.z3()),
            len: bv,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Constant {
    Stack(StackValue),
    Heap(HeapValue),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SimpleRef {
    Class { name: String },
    Array { ty: ArrayType },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArrayType {
    Int,
    Float,
    Byte,
    Char,
    Short,
    Boolean,
    Long,
    Double,
}

impl ArrayType {
    pub fn to_simple_type(&self) -> SimpleType {
        match self {
            ArrayType::Int => SimpleType::Int,
            ArrayType::Float => SimpleType::Float,
            ArrayType::Byte => SimpleType::Byte,
            ArrayType::Char => SimpleType::Char,
            ArrayType::Short => SimpleType::Short,
            ArrayType::Boolean => SimpleType::Boolean,
            ArrayType::Long => SimpleType::Long,
            ArrayType::Double => SimpleType::Double,
        }
    }

    pub fn z3_sort(&self) -> z3::Sort {
        match self {
            ArrayType::Int => z3::Sort::bitvector(32),
            ArrayType::Float => z3::Sort::float32(),
            ArrayType::Byte => z3::Sort::bitvector(8),
            ArrayType::Char => z3::Sort::bitvector(16),
            ArrayType::Short => z3::Sort::bitvector(16),
            ArrayType::Boolean => z3::Sort::bitvector(1),
            ArrayType::Long => z3::Sort::bitvector(64),
            // Check that this is correct?
            ArrayType::Double => z3::Sort::double(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SimpleType {
    Int,
    Long,
    Float,
    Double,
    Byte,
    Char,
    Short,
    Boolean,
    SimpleRef(SimpleRef),
}

impl SimpleType {
    pub fn to_array_type(&self) -> ArrayType {
        match self {
            SimpleType::Int => ArrayType::Int,
            SimpleType::Float => ArrayType::Float,
            SimpleType::Byte => ArrayType::Byte,
            SimpleType::Char => ArrayType::Char,
            SimpleType::Short => ArrayType::Short,
            SimpleType::Boolean => ArrayType::Boolean,
            SimpleType::SimpleRef(_) => panic!("Cannot convert simpleref to ArrayType"),
            SimpleType::Long => ArrayType::Long,
            SimpleType::Double => ArrayType::Double,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackType {
    Int,
    Float,
    Ref,
    Long,
    Double,
}
