use crate::{
    ArrayType,
    java_class::MethodId,
    java_types::{SimpleRef, StackType, StackValue},
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
        value: StackValue,
    },
    Return {
        ty: Option<StackType>,
    },
    Get,
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
            Op::Div => lhs.checked_div(rhs),
            Op::Rem => Some(lhs.wrapping_rem(rhs)),
        }
    }

    pub fn op_z3(&self, lhs: &z3::ast::BV, rhs: &z3::ast::BV) -> z3::ast::BV {
        match self {
            Op::Add => lhs.bvadd(rhs),
            Op::Sub => lhs.bvsub(rhs),
            Op::Mul => lhs.bvmul(rhs),
            Op::Div => lhs.bvsdiv(rhs),
            Op::Rem => lhs.bvsrem(rhs),
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
