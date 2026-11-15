use crate::{
    java_class::MethodId,
    java_types::{HeapValue, StackType, StackValue},
};

pub enum Either {
    State(State),
    Result(ExeResult),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExeResult {
    Ok(Option<StackValue>),
    AssertErr,
    OutOfBounds,
    NullPointer,
    Div0,
    NoHalt,
    DidNotFinish,
}

#[derive(Clone, Debug)]
pub struct State {
    pub heap: Heap,
    pub frames: Vec<Frame>,
}

impl State {
    pub fn new(program_counter: ProgramCounter, input: Vec<StackValue>, heap: Heap) -> Self {
        Self {
            heap,
            frames: vec![Frame::new(program_counter, input)],
        }
    }

    pub fn program_counter(&self) -> ProgramCounter {
        let Some(frame) = self.frames.last() else {
            panic!()
        };
        frame.program_counter.clone()
    }
}

#[derive(Clone, Debug)]
pub struct Heap {
    pub heap: Vec<HeapValue>,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub stack: Vec<StackValue>,
    pub locals: Vec<Option<StackValue>>,
    pub program_counter: ProgramCounter,
}

impl Frame {
    pub fn new(program_counter: ProgramCounter, args: Vec<StackValue>) -> Self {
        // Long and double take two local slots, the second one is unusable
        let mut locals = Vec::with_capacity(args.len());
        for value in args {
            let is_wide = value.is_wide();
            locals.push(Some(value));
            if is_wide {
                locals.push(None);
            }
        }
        Self {
            stack: vec![],
            locals,
            program_counter,
        }
    }

    pub fn pc(&self) -> usize {
        self.program_counter.idx
    }

    pub fn set_pc(&mut self, target: u32) {
        self.program_counter.idx = target as usize;
    }

    pub fn increment_pc(&mut self) {
        self.program_counter.idx += 1;
    }

    pub fn dup(&mut self, words: u32) {
        if words == 1 {
            self.stack.push(self.stack[self.stack.len() - 1].clone());
        } else if words == 2 {
            if self.stack[self.stack.len() - 1].is_wide() {
                self.stack.push(self.stack[self.stack.len() - 1].clone());
            } else {
                self.stack.push(self.stack[self.stack.len() - 2].clone());
                self.stack.push(self.stack[self.stack.len() - 2].clone());
            }
        } else {
            panic!()
        }
    }

    pub(crate) fn push(&mut self, value: StackValue) {
        self.stack.push(value);
    }

    pub(crate) fn load(&mut self, ty: StackType, index: u32) {
        let local = self.locals[index as usize].clone().unwrap();
        assert!(local.get_type() == ty);

        self.stack.push(local);
    }

    pub(crate) fn store(&mut self, ty: StackType, index: u32) {
        let value = self.stack.pop().unwrap();
        assert!(value.get_type() == ty);

        let index = index as usize;
        let slots = if value.is_wide() { 2 } else { 1 };
        self.locals
            .resize((index + slots).max(self.locals.len()), None);
        if slots == 2 {
            self.locals[index + 1] = None;
        }
        self.locals[index] = Some(value);
    }
}

#[derive(Clone, Debug)]
pub struct ProgramCounter {
    pub class: String,
    pub method: MethodId,
    pub idx: usize,
}
