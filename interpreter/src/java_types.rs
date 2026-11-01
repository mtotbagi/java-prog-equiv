use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub enum StackValue {
    Int(i32),
    Float(f32),
    Ref(Option<u32>),
}

impl StackValue {
    pub fn get_type(&self) -> StackType {
        match self {
            StackValue::Int(_) => StackType::Int,
            StackValue::Float(_) => StackType::Float,
            StackValue::Ref(_) => StackType::Ref,
        }
    }

    pub fn to_heap_value(&self) -> HeapValue {
        match self {
            StackValue::Int(i) => HeapValue::Int(*i),
            StackValue::Float(f) => HeapValue::Float(*f),
            StackValue::Ref(_) => panic!("Ref can't be turned into a heap value"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum HeapValue {
    Int(i32),
    Float(f32),
    Byte(u8),
    Char(u16),
    Short(i16),
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
            HeapValue::Int(i) => StackValue::Int(*i),
            HeapValue::Float(f) => StackValue::Float(*f),
            HeapValue::Byte(b) => StackValue::Int(*b as i32),
            HeapValue::Short(s) => StackValue::Int(*s as i32),
            HeapValue::Char(c) => StackValue::Int(*c as i32),
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
