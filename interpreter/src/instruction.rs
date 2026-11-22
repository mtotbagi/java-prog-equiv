use z3::ast::{BV, RoundingMode};

use crate::{
    ArrayType, Constant, SimpleType,
    java_class::MethodId,
    java_types::{SimpleRef, StackType},
};

#[derive(Clone, Debug)]
pub enum Instruction {
    Ifz {
        cond: Cond,
        target: u32,
    },
    Load {
        ty: StackType,
        index: u32,
    },
    Store {
        ty: StackType,
        index: u32,
    },
    Push {
        value: Constant,
    },
    Return {
        ty: Option<StackType>,
    },
    Get {
        is_static: bool,
        class_name: String,
        field_name: String,
        field_ty: SimpleType,
    },
    New {
        class: String,
    },
    Dup {
        words: u32,
    },
    Invoke {
        access: Access,
        method_id: MethodId,
        simple_ref: SimpleRef,
    },
    Throw,
    Binary {
        op: Op,
        ty: StackType,
    },
    Neg {
        ty: StackType,
    },
    If {
        cond: Cond,
        target: u32,
    },
    Goto {
        target: u32,
    },
    Placeholder,
    NewArray {
        dim: u32,
        ty: ArrayType,
    },
    ArrayStore {
        ty: ArrayType,
    },
    ArrayLength,
    ArrayLoad {
        ty: ArrayType,
    },
    Incr {
        index: u32,
        amount: i32,
    },
    NoOp,
    Put {
        is_static: bool,
        class_name: String,
        field_name: String,
        field_ty: SimpleType,
    },
    CompareFloat {
        on_nan: i32,
        // If true, doubles are compared, else floats
        double: bool,
    },
    CompareLong,
    DupX2,
    BitOpr {
        op: BitOp,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum BitOp {
    Shl,
    Shr,
    Ushr,
    And,
    Or,
    Xor,
}

impl BitOp {
    pub fn op_int(&self, lhs: i32, rhs: i32) -> i32 {
        match self {
            BitOp::Shl => lhs.wrapping_shl(rhs as u32),
            BitOp::Shr => lhs.wrapping_shr(rhs as u32),
            BitOp::Ushr => (lhs as u32).wrapping_shr(rhs as u32) as i32,
            BitOp::And => lhs & rhs,
            BitOp::Or => lhs | rhs,
            BitOp::Xor => lhs ^ rhs,
        }
    }

    pub fn op_long(&self, lhs: i64, rhs: i64) -> i64 {
        match self {
            BitOp::Shl => lhs.wrapping_shl(rhs as u32),
            BitOp::Shr => lhs.wrapping_shr(rhs as u32),
            BitOp::Ushr => (lhs as u64).wrapping_shr(rhs as u32) as i64,
            BitOp::And => lhs & rhs,
            BitOp::Or => lhs | rhs,
            BitOp::Xor => lhs ^ rhs,
        }
    }

    pub fn op_z3_bv(&self, lhs: &BV, rhs: &BV) -> BV {
        match self {
            BitOp::Shl => lhs.bvshl(rhs),
            BitOp::Shr => lhs.bvashr(rhs),
            BitOp::Ushr => lhs.bvlshr(rhs),
            BitOp::And => lhs.bvand(rhs),
            BitOp::Or => lhs.bvor(rhs),
            BitOp::Xor => lhs.bvxor(rhs),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl Op {
    pub fn op_int(&self, lhs: i32, rhs: i32) -> Option<i32> {
        match self {
            Op::Add => Some(lhs.wrapping_add(rhs)),
            Op::Sub => Some(lhs.wrapping_sub(rhs)),
            Op::Mul => Some(lhs.wrapping_mul(rhs)),
            Op::Div => (rhs != 0).then(|| lhs.wrapping_div(rhs)),
            Op::Rem => (rhs != 0).then(|| lhs.wrapping_rem(rhs)),
        }
    }

    pub fn op_long(&self, lhs: i64, rhs: i64) -> Option<i64> {
        match self {
            Op::Add => Some(lhs.wrapping_add(rhs)),
            Op::Sub => Some(lhs.wrapping_sub(rhs)),
            Op::Mul => Some(lhs.wrapping_mul(rhs)),
            Op::Div => (rhs != 0).then(|| lhs.wrapping_div(rhs)),
            Op::Rem => (rhs != 0).then(|| lhs.wrapping_rem(rhs)),
        }
    }

    pub fn op_f32(&self, lhs: f32, rhs: f32) -> f32 {
        match self {
            Op::Add => lhs + rhs,
            Op::Sub => lhs - rhs,
            Op::Mul => lhs * rhs,
            Op::Div => lhs / rhs,
            Op::Rem => lhs % rhs,
        }
    }

    pub fn op_f64(&self, lhs: f64, rhs: f64) -> f64 {
        match self {
            Op::Add => lhs + rhs,
            Op::Sub => lhs - rhs,
            Op::Mul => lhs * rhs,
            Op::Div => lhs / rhs,
            Op::Rem => lhs % rhs,
        }
    }

    pub fn op_z3_bv(&self, lhs: &z3::ast::BV, rhs: &z3::ast::BV) -> z3::ast::BV {
        match self {
            Op::Add => lhs.bvadd(rhs),
            Op::Sub => lhs.bvsub(rhs),
            Op::Mul => lhs.bvmul(rhs),
            Op::Div => lhs.bvsdiv(rhs),
            Op::Rem => lhs.bvsrem(rhs),
        }
    }

    pub fn op_z3_float(&self, lhs: &z3::ast::Float, rhs: &z3::ast::Float) -> z3::ast::Float {
        let rm = RoundingMode::round_nearest_ties_to_away();
        match self {
            Op::Add => lhs.add_with_rounding_mode(rhs, &rm),
            Op::Sub => lhs.sub_with_rounding_mode(rhs, &rm),
            Op::Mul => lhs.mul_with_rounding_mode(rhs, &rm),
            Op::Div => lhs.div_with_rounding_mode(rhs, &rm),
            Op::Rem => lhs.rem(rhs),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Access {
    Special,
    Virtual,
    Static,
    Interface,
    Dynamic,
}

#[derive(Clone, Debug)]
pub enum Cond {
    Ne,
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
    Is,
    IsNot,
}

impl Cond {
    pub fn cmp_i32(&self, lhs: i32, rhs: i32) -> bool {
        match self {
            Cond::Ne => lhs != rhs,
            Cond::Eq => lhs == rhs,
            Cond::Lt => lhs < rhs,
            Cond::Le => lhs <= rhs,
            Cond::Gt => lhs > rhs,
            Cond::Ge => lhs >= rhs,
            Cond::Is => lhs == rhs,
            Cond::IsNot => lhs != rhs,
        }
    }

    pub fn cmp_ref(&self, lhs: Option<u32>, rhs: Option<u32>) -> bool {
        match self {
            Cond::Is => lhs == rhs,
            Cond::IsNot => lhs != rhs,
            _ => panic!("Invalid condition for references!"),
        }
    }

    pub fn cmp_z3_ref(
        &self,
        lhs: (&z3::ast::Bool, &z3::ast::BV),
        rhs: (&z3::ast::Bool, &z3::ast::BV),
    ) -> z3::ast::Bool {
        match self {
            Cond::Is => z3::ast::Bool::and(&[
                lhs.0.eq(rhs.0),
                lhs.0
                    .eq(z3::ast::Bool::from_bool(true))
                    .implies(lhs.1.eq(rhs.1)),
            ]),
            Cond::IsNot => Cond::Is.cmp_z3_ref(lhs, rhs).not(),
            _ => panic!("Invalid condition"),
        }
    }

    pub fn cmp_z3_signed(&self, lhs: &z3::ast::BV, rhs: &z3::ast::BV) -> z3::ast::Bool {
        match self {
            Cond::Ne => lhs.ne(rhs),
            Cond::Eq => lhs.eq(rhs),
            Cond::Lt => lhs.bvslt(rhs),
            Cond::Le => lhs.bvsle(rhs),
            Cond::Gt => lhs.bvsgt(rhs),
            Cond::Ge => lhs.bvsge(rhs),
            Cond::Is => lhs.eq(rhs),
            Cond::IsNot => lhs.ne(rhs),
        }
    }
}
