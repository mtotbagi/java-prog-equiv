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

    pub fn z3(&self) -> z3::ast::Dynamic {
        match self {
            ArrayValue::Int(_, bv)
            | ArrayValue::Byte(_, bv)
            | ArrayValue::Char(_, bv)
            | ArrayValue::Short(_, bv)
            | ArrayValue::Boolean(_, bv) => z3::ast::Dynamic::from_ast(bv),
            ArrayValue::Float(_) => todo!(),
        }
    }

    pub fn swap_sym_val(&self, sym_val: z3::ast::Dynamic) -> ArrayValue {
        match self {
            ArrayValue::Int(i, _) => ArrayValue::Int(*i, sym_val.as_bv().unwrap()),
            ArrayValue::Float(f) => ArrayValue::Float(*f),
            ArrayValue::Byte(b, _) => ArrayValue::Byte(*b, sym_val.as_bv().unwrap()),
            ArrayValue::Char(c, _) => ArrayValue::Char(*c, sym_val.as_bv().unwrap()),
            ArrayValue::Short(s, _) => ArrayValue::Short(*s, sym_val.as_bv().unwrap()),
            ArrayValue::Boolean(b, _) => ArrayValue::Boolean(*b, sym_val.as_bv().unwrap()),
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
            ArrayType::Float => ArrayValue::Float(0.0),
            ArrayType::Byte => ArrayValue::Byte(0, z3::ast::BV::from_i64(0, 8)),
            ArrayType::Char => ArrayValue::Char(0, z3::ast::BV::from_i64(0, 16)),
            ArrayType::Short => ArrayValue::Short(0, z3::ast::BV::from_i64(0, 16)),
            ArrayType::Boolean => ArrayValue::Boolean(false, z3::ast::BV::from_i64(0, 1)),
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

    pub fn z3_sort(&self) -> z3::Sort {
        match self {
            ArrayType::Int => z3::Sort::bitvector(32),
            ArrayType::Float => z3::Sort::float32(),
            ArrayType::Byte => z3::Sort::bitvector(8),
            ArrayType::Char => z3::Sort::bitvector(16),
            ArrayType::Short => z3::Sort::bitvector(16),
            ArrayType::Boolean => z3::Sort::bitvector(1),
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
