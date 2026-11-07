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
        max_steps: u32,
        max_runs: u32
    ) -> Vec<(Vec<(StackValue, Heap)>, Vec<z3::ast::Bool>, ExeResult)> {
        

    }
}