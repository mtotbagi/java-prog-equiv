use core::panic;
use std::{eprint, print, println};

use z3::{SatResult, Solver, ast::BV};

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
        while let Some(input) = input_stack.pop() {
            let (_, path, res) = self.interpreter.interpret(method, input.0.clone(), iter);
            output.push((input.0, path.clone(), res));

            for (i,c) in path[(input.1)..].iter().enumerate() {
                println!("{i}");
                println!("{}", path.len());
                
                let c = c.not();
                let s = Solver::new();
                
                println!("{c}");

                for j in 0..i {
                    s.assert(path[j].clone());
                }
                s.assert(c);
                println!("{s}");

                if s.check() == SatResult::Sat {
                    let new_input = s.get_model().unwrap();
                    println!("{new_input}");
                    //.eval(ast, true).unwrap();
                    //input_stack.push((new_input,i));
                }
                

            }
        }
        
        
        // //Empty path
        // if path.is_empty() {
        //     println!("no branches on this path");
        //     return;
        // }
        
        // println!("result: {res:?}");
        // for constraint in &path {
        //     println!("condition: {constraint}");
        // }

        // //Negate the last condition
        // let no_constraints = path.len();
        // let last_constraint = path[no_constraints - 1].clone();
        // path[no_constraints - 1] = last_constraint.not();

        // //Solve for the new path trying to get a new input
        // let s = Solver::new();
        // for constraint in path {
        //     s.assert(constraint);
        // }

        // if s.check() == SatResult::Sat {
        //     println!("{}", s.get_model().unwrap());
        // };
        
        output
    }

}