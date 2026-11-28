use z3::ast::{BV, Bool, RoundingMode};

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
    /// Primitive conversion (i2l, i2d, l2i, d2f, i2s, ...)
    Cast {
        from: StackType,
        to: SimpleType,
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
            BitOp::And => lhs & rhs,
            BitOp::Or => lhs | rhs,
            BitOp::Xor => lhs ^ rhs,
            _ => panic!("Invalid bit opr"),
        }
    }

    pub fn op_long_int(&self, lhs: i64, rhs: i32) -> i64 {
        match self {
            BitOp::Shl => lhs.wrapping_shl(rhs as u32),
            BitOp::Shr => lhs.wrapping_shr(rhs as u32),
            BitOp::Ushr => (lhs as u64).wrapping_shr(rhs as u32) as i64,
            _ => panic!("Invalid bit opr"),
        }
    }

    /// Performs bit operation on symbolic bitvectors according to the corresponding Java operations
    pub fn op_z3_bv(&self, lhs: &BV, rhs: &BV) -> BV {
        let shift_amount = {
            let width = lhs.get_size();
            let masked = rhs.bvand(BV::from_u64(u64::from(width - 1), rhs.get_size()));
            masked.zero_ext(width - rhs.get_size())
        };
        match self {
            BitOp::Shl => lhs.bvshl(shift_amount),
            BitOp::Shr => lhs.bvashr(shift_amount),
            BitOp::Ushr => lhs.bvlshr(shift_amount),
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
        // Java rounds to nearest, ties to even
        let rm = RoundingMode::round_nearest_ties_to_even();
        match self {
            Op::Add => lhs.add_with_rounding_mode(rhs, &rm),
            Op::Sub => lhs.sub_with_rounding_mode(rhs, &rm),
            Op::Mul => lhs.mul_with_rounding_mode(rhs, &rm),
            Op::Div => lhs.div_with_rounding_mode(rhs, &rm),
            Op::Rem => {
                // Java's `%` truncates the quotient (like C's fmod), z3's IEEE remainder
                // rounds it to nearest. They differ by |rhs| when the IEEE remainder's sign
                // is not the sign of lhs, and the corrected value is exact.
                let r = lhs.rem(rhs);
                let abs = rhs.unary_abs();
                let too_low = Bool::and(&[lhs.is_positive(), r.is_negative()]);
                let too_high = Bool::and(&[lhs.is_negative(), r.is_positive()]);
                too_low.ite(
                    &r.add_with_rounding_mode(&abs, &rm),
                    &too_high.ite(&r.sub_with_rounding_mode(&abs, &rm), &r),
                )
            }
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
