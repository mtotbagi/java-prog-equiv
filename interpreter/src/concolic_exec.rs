use z3::{
    Model, Params, SatResult, Solver,
    ast::{BV, Bool, Float},
};

use crate::*;

pub type Run = ((Vec<StackValue>, Heap), Vec<z3::ast::Bool>, ExeResult);
pub const SOLVER_TIMEOUT_MS: u32 = 100;
pub struct ConcolicExecutor {
    interpreter: Interpreter,
}

impl ConcolicExecutor {
    pub fn new(class: Class) -> Self {
        ConcolicExecutor {
            interpreter: Interpreter::new(class),
        }
    }

    pub fn run(&self, method: &Method, iter: u32, max_runs: u32) -> Vec<Run> {
        let mut output: Vec<Run> = Vec::new();
        let mut params = Params::new();
        params.set_u32("timeout", SOLVER_TIMEOUT_MS);
        // The first concrete input is all zeros, with one named symbolic variable per
        // parameter (see `initial_input`).
        let initial_input = self.initial_input(method).unwrap_or_else(|e| panic!("{e}"));
        let mut input_stack = vec![(initial_input, 0)];

        // run interpreter
        while let Some((input, depth)) = input_stack.pop() {
            if output.len() as u32 >= max_runs {
                break;
            }
            let (_, path, res) = self.interpreter.interpret(method, input.clone(), iter);

            // Negate every condition from some depth n
            for i in depth..path.len() {
                let s = Solver::new();
                s.set_params(&params);
                // Keep the path the same until the negated condition
                for c in &path[..i] {
                    s.assert(c);
                }
                s.assert(path[i].not());

                if s.check() == SatResult::Sat {
                    let model = s.get_model().unwrap();
                    input_stack.push((next_input(&model, &input), i + 1));
                }
            }

            output.push((input, path, res));
        }

        output
    }

    pub fn initial_input(&self, method: &Method) -> Result<(Vec<StackValue>, Heap), String> {
        let mut args = Vec::new();
        let mut heap = Vec::new();
        if !method.is_static {
            args.push(StackValue::Ref(
                Some(0),
                Bool::from_bool(true),
                BV::from_u64(0, 32),
            ));
            heap.push(self.interpreter.class().new_object());
        }
        for (i, ty) in method.id.params.iter().enumerate() {
            let name = format!("arg{i}");
            let name = name.as_str();
            let value = match ty {
                SimpleType::Int => StackValue::Int(0, BV::new_const(name, 32)),
                SimpleType::Byte => StackValue::Int(0, BV::new_const(name, 8).sign_ext(24)),
                SimpleType::Short => StackValue::Int(0, BV::new_const(name, 16).sign_ext(16)),
                SimpleType::Char => StackValue::Int(0, BV::new_const(name, 16).zero_ext(16)),
                SimpleType::Boolean => StackValue::Int(0, BV::new_const(name, 1).zero_ext(31)),
                SimpleType::Long => StackValue::Long(0, BV::new_const(name, 64)),
                SimpleType::Float => StackValue::Float(0.0, Float::new_const_float32(name)),
                SimpleType::Double => StackValue::Double(0.0, Float::new_const_double(name)),
                SimpleType::SimpleRef(_) => {
                    return Err(format!(
                        "method argument type not implemented: {ty:?} (argument {i})"
                    ));
                }
            };
            args.push(value);
        }
        Ok((args, Heap { heap }))
    }
}

/// The same input, but with the values the model assigns to the symbolic variables.
pub fn next_input(model: &Model, old_input: &(Vec<StackValue>, Heap)) -> (Vec<StackValue>, Heap) {
    let values = old_input
        .0
        .iter()
        .map(|value| match value {
            StackValue::Int(old, bv) => {
                let solved = eval_bits(model, bv).map_or(*old, |bits| bits as u32 as i32);
                StackValue::Int(solved, bv.clone())
            }
            StackValue::Long(old, bv) => {
                let solved = eval_bits(model, bv).map_or(*old, |bits| bits as i64);
                StackValue::Long(solved, bv.clone())
            }
            StackValue::Float(old, f) => {
                let solved =
                    eval_float_bits(model, f).map_or(*old, |bits| f32::from_bits(bits as u32));
                StackValue::Float(solved, f.clone())
            }
            StackValue::Double(old, f) => {
                let solved = eval_float_bits(model, f).map_or(*old, f64::from_bits);
                StackValue::Double(solved, f.clone())
            }
            other => other.clone(),
        })
        .collect();
    (values, old_input.1.clone())
}

/// Value of a bitvector in the model, as unsigned bits.
fn eval_bits(model: &Model, bv: &BV) -> Option<u64> {
    model.eval(bv, true)?.as_u64()
}

/// Value of a floating point term in the model, as its IEEE-754 bits.
fn eval_float_bits(model: &Model, f: &Float) -> Option<u64> {
    eval_bits(model, &f.to_ieee_bv())
}
