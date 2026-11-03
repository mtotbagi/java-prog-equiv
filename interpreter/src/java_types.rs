use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum StackValue {
    Int(i32, z3::ast::BV),
    Float(f32),
    Ref(Option<u32>),
}

impl StackValue {
    pub fn get_type(&self) -> StackType {
        match self {
            StackValue::Int(_, _) => StackType::Int,
            StackValue::Float(_) => StackType::Float,
            StackValue::Ref(_) => StackType::Ref,
        }
    }

    pub fn to_heap_value(&self) -> HeapValue {
        match self {
            StackValue::Int(i, bv) => HeapValue::Int(*i, bv.clone()),
            StackValue::Float(f) => HeapValue::Float(*f),
            StackValue::Ref(_) => panic!("Ref can't be turned into a heap value"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum HeapValue {
    Int(i32, z3::ast::BV),
    Float(f32),
    Byte(u8, z3::ast::BV),
    Char(u16, z3::ast::BV),
    Short(i16, z3::ast::BV),
    Array {
        ty: SimpleType,
        values: Vec<HeapValue>,
    },
    Object {
        name: String,
        fields: HashMap<String, HeapValue>,
    },
}

impl HeapValue {
    pub fn to_stack_value(&self) -> StackValue {
        match self {
            HeapValue::Int(i, bv) => StackValue::Int(*i, bv.clone()),
            HeapValue::Float(f) => StackValue::Float(*f),
            HeapValue::Byte(b, bv) => StackValue::Int(*b as i32, bv.zero_ext(24)),
            HeapValue::Short(s, bv) => StackValue::Int(*s as i32, bv.sign_ext(16)),
            HeapValue::Char(c, bv) => StackValue::Int(*c as i32, bv.zero_ext(16)),
            _ => panic!("Can't convert to StackValue"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum SimpleRef {
    Class { name: String },
    Array { ty: SimpleType },
}

#[derive(Clone, Debug)]
pub enum SimpleType {
    Int,
    // Long,
    Float,
    // Double,
    Byte,
    Char,
    Short,
    Boolean,
    SimpleRef(Box<SimpleRef>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackType {
    Int,
    Float,
    Ref,
}
