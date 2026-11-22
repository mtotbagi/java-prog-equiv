use std::collections::HashMap;

use z3::{
    Model, SatResult, Solver,
    ast::{BV, Bool, Float},
};

use crate::*;

/// A concolic run: the concrete input, the path condition and the result.
pub type Run = ((Vec<StackValue>, Heap), Vec<z3::ast::Bool>, ExeResult);

pub struct ConcolicExecutor {
    interpreter: Interpreter,
    class_name: String,
}

impl ConcolicExecutor {
    pub fn new(class: Class) -> Self {
        ConcolicExecutor {
            class_name: class.name.clone(),
            interpreter: Interpreter::new(class),
        }
    }

    pub fn run(&self, method: &Method, iter: u32, max_runs: u32) -> Vec<Run> {
        let mut output: Vec<Run> = Vec::new();

        // The first concrete input is all zeros, with one named symbolic variable per
        // parameter (see `initial_input`).
        let initial_input =
            initial_input(&self.class_name, method).unwrap_or_else(|e| panic!("{e}"));
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
}

/// Builds the first concrete input (all zeros / false / NUL) for `method`.
///
/// The symbolic side of argument `i` is a Z3 constant *named* `arg{i}` (not a fresh
/// one). Two executors for two versions of the same method therefore talk about the
/// SAME input variables, so their path conditions can be combined in one solver query
/// (this is what `prog-equiv` needs).
///
/// Narrow types are modelled like the JVM does: the variable has the narrow width and
/// is widened to 32 bits (sign extension for byte/short, zero extension for char and
/// boolean), so that e.g. a `byte` can never be 200.
///
/// For an instance method, local 0 is `this`: a non-null object of `class_name` that is
/// the first (and only) object on the heap. Its fields are empty, so a method that
/// reads an instance field still fails inside the interpreter.
///
/// Returns an error for parameter types that are not supported yet (references).
pub fn initial_input(class_name: &str, method: &Method) -> Result<(Vec<StackValue>, Heap), String> {
    let mut args = Vec::new();
    let mut heap = Vec::new();
    if !method.is_static {
        args.push(StackValue::Ref(
            Some(0),
            Bool::from_bool(true),
            BV::from_u64(0, 32),
        ));
        heap.push(HeapValue::Object {
            name: class_name.to_string(),
            fields: HashMap::new(),
        });
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

/// The same input, but with the values the model assigns to the symbolic variables.
fn next_input(model: &Model, old_input: &(Vec<StackValue>, Heap)) -> (Vec<StackValue>, Heap) {
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
///
/// Note: `as_u64`, not `as_i64`. Z3 numerals are unsigned, so a negative `long`
/// does not fit in an `i64` numeral and `as_i64` would return `None`.
fn eval_bits(model: &Model, bv: &BV) -> Option<u64> {
    model.eval(bv, true)?.as_u64()
}

/// Value of a floating point term in the model, as its IEEE-754 bits.
fn eval_float_bits(model: &Model, f: &Float) -> Option<u64> {
    eval_bits(model, &f.to_ieee_bv())
}