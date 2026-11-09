use core::panic;
use std::{println};

use z3::{SatResult, Solver, Model};

use crate::*;

pub struct ConcolicExecutor {
    interpreter: Interpreter,
}

impl ConcolicExecutor {
    pub fn new(class: Class) -> Self {
        ConcolicExecutor {
            interpreter: Interpreter::new(class),
        }
    }

    pub fn run(
        &self, 
        method: &Method, 
        iter: u32,
        max_runs: u32
    ) -> Vec<((Vec<StackValue>,Heap), Vec<z3::ast::Bool>, ExeResult)> {
        // output
        let mut output : Vec<((Vec<StackValue>,Heap), Vec<z3::ast::Bool>, ExeResult)> = Vec::new();

        // generate initial concrete input from method input types
        let concrete_inputs : Vec<&str> = 
            method.id.params.iter().map(|t| 
                match t {
                    SimpleType::Int => "0",
                    _ => panic!("method argument type not implemented")
                }
            ).collect();
        let concrete_input : String = format!("({})", concrete_inputs.join(","));
        let initial_input = parse_input(method, &concrete_input);
        let mut input_stack = vec![(initial_input, 0)];

        // run interpreter
        while let Some((input,depth)) = input_stack.pop() {
            if output.len() as u32 >= max_runs{
                break;
            }
            let (_, path, res) = self.interpreter.interpret(method, input.clone(), iter);
            
            //Negate every input from some depth n
            for i in depth..path.len() {
                let s = Solver::new();
                //Keep the inputs the same until the negated input
                for c in &path[..i] {
                    s.assert(c);
                }
                s.assert(path[i].not());

                if s.check() == SatResult::Sat {
                    let model = s.get_model().unwrap();
                    input_stack.push((next_input(&model, &input), i+1));
                }
            }

            output.push((input, path.clone(), res));
        }
        
        output
    }

}

fn next_input(model: &Model, old_input: &(Vec<StackValue>, Heap)) -> (Vec<StackValue>, Heap) {
    let values = old_input
        .0
        .iter()
        .map(|value| match value {
            StackValue::Int(_, bv) => {
                let solved = model.eval(bv, true).unwrap().as_i64().unwrap() as i32;
                StackValue::Int(solved, bv.clone())
            }
            other => other.clone(),
        })
        .collect();
    (values, old_input.1.clone())
}
