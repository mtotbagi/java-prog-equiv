use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum StackValue {
    Int(i32, z3::ast::BV),
    Float(f32),
    // The symbolic bool true iff ref is Some
    Ref(Option<u32>, z3::ast::Bool, z3::ast::BV),
}

impl StackValue {
    pub fn get_type(&self) -> StackType {
        match self {
            StackValue::Int(_, _) => StackType::Int,
            StackValue::Float(_) => StackType::Float,
            StackValue::Ref(_, _, _) => StackType::Ref,
        }
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
            (StackValue::Float(f), ArrayType::Float) => ArrayValue::Float(*f),
            _ => panic!("Cannot convert {:?} to {:?}", self, ty),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ArrayValue {
    Int(i32, z3::ast::BV),
    Float(f32),
    Byte(u8, z3::ast::BV),
    Char(u16, z3::ast::BV),
    Short(i16, z3::ast::BV),
    Boolean(bool, z3::ast::BV),
}

impl ArrayValue {
    pub fn to_stack_value(&self) -> StackValue {
        match self {
            ArrayValue::Int(i, bv) => StackValue::Int(*i, bv.clone()),
            ArrayValue::Float(f) => StackValue::Float(*f),
            ArrayValue::Byte(i, bv) => StackValue::Int(*i as i32, bv.zero_ext(24)),
            ArrayValue::Char(i, bv) => StackValue::Int(*i as i32, bv.zero_ext(16)),
            ArrayValue::Short(i, bv) => StackValue::Int(*i as i32, bv.sign_ext(16)),
            ArrayValue::Boolean(i, bv) => StackValue::Int(*i as i32, bv.zero_ext(31)),
        }
    }
}

#[derive(Clone, Debug)]
pub enum HeapValue {
    Int(i32, z3::ast::BV),
    Ref(Option<u32>, z3::ast::Bool, z3::ast::BV),
    Float(f32),
    Byte(u8, z3::ast::BV),
    Char(u16, z3::ast::BV),
    Short(i16, z3::ast::BV),
    Boolean(bool, z3::ast::BV),
    Array {
        ty: ArrayType,
        values: Vec<ArrayValue>,
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
            HeapValue::Ref(r, sym_is_null, sym_bv) => {
                StackValue::Ref(*r, sym_is_null.clone(), sym_bv.clone())
            }
            _ => panic!("Can't convert to StackValue"),
        }
    }

    pub fn symbolic_values(&self) -> Vec<z3::ast::Dynamic> {
        // Important: deterministic order!!!
        todo!()
    }
}

#[derive(Clone, Debug)]
pub enum SimpleRef {
    Class { name: String },
    Array { ty: ArrayType },
}

#[derive(Clone, Copy, Debug)]
pub enum ArrayType {
    Int,
    Float,
    Byte,
    Char,
    Short,
    Boolean,
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
        }
    }
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
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackType {
    Int,
    Float,
    Ref,
}
