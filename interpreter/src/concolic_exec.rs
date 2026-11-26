//! Implements a concolic executor as an iterator returning the possible paths a Java program can take.
//! Some paths (which cannot be solved by z3) will be discarded
//!
//! Internally uses rayon threadpool to preemtively compute items which returned by the iterator when needed
//!
//! As z3 is not thread-safe, no symbolic data is being passed between threads,
//! they are recomputed (cheaply) by interpreting the input they were generated from the first time.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::Relaxed},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use z3::{
    Model, Params, SatResult, Solver,
    ast::{BV, Bool, Float},
};

use crate::*;

pub type Run = ((Vec<StackValue>, Heap), Vec<z3::ast::Bool>, ExeResult);
pub const SOLVER_TIMEOUT_MS: u32 = 100;

pub fn new_solver() -> Solver {
    Solver::new_for_logic("QF_FPBV").unwrap()
}

#[derive(Debug, Clone)]
pub struct ConcolicExecutor {
    interpreter: Interpreter,
}

impl ConcolicExecutor {
    pub fn new(class: Class) -> Self {
        ConcolicExecutor {
            interpreter: Interpreter::new(class),
        }
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

    pub fn into_iter(
        self,
        method: &Method,
        step_count: u32,
        deadline: Instant,
        workers: usize,
    ) -> impl Iterator<Item = Run> {
        let initial_input = self.initial_input(method).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let explorer = Explorer {
            executor: self.clone(),
            method: method.clone(),
            step_count,
            deadline,
            stop: stop.clone(),
            tx,
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers.max(1))
            .build()
            .unwrap();
        let producer = thread::spawn(move || pool.scope(|s| explorer.visit(s, WorkItem::Root)));
        ConcolicPathsIter {
            rx,
            initial_input,
            interpreter: self.interpreter,
            method: method.clone(),
            step_count,
            deadline,
            stop,
            producer: Some(producer),
        }
    }
}

type Input = (Vec<StackValue>, Heap);

/// The concrete part of an input value
/// it holds no z3 terms, can be sent between threads.
#[derive(Clone, Debug)]
enum ConcreteArg {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    /// Special case for the `this` argument on non-static method in java.
    /// We cannot handle other types of reference inputs in the entry method
    Ref,
}

type ConcreteInput = Vec<ConcreteArg>;

fn to_concrete((args, _): &Input) -> ConcreteInput {
    args.iter()
        .map(|value| match value {
            StackValue::Int(v, _) => ConcreteArg::Int(*v),
            StackValue::Long(v, _) => ConcreteArg::Long(*v),
            StackValue::Float(v, _) => ConcreteArg::Float(*v),
            StackValue::Double(v, _) => ConcreteArg::Double(*v),
            StackValue::Ref(..) => ConcreteArg::Ref,
        })
        .collect()
}

fn from_concrete(initial: &Input, concrete: &ConcreteInput) -> Input {
    let args = initial
        .0
        .iter()
        .zip(concrete)
        .map(|(value, c)| match (value, c) {
            (StackValue::Int(_, bv), ConcreteArg::Int(v)) => StackValue::Int(*v, bv.clone()),
            (StackValue::Long(_, bv), ConcreteArg::Long(v)) => StackValue::Long(*v, bv.clone()),
            (StackValue::Float(_, f), ConcreteArg::Float(v)) => StackValue::Float(*v, f.clone()),
            (StackValue::Double(_, f), ConcreteArg::Double(v)) => StackValue::Double(*v, f.clone()),
            (other, _) => other.clone(),
        })
        .collect();
    (args, initial.1.clone())
}

enum WorkItem {
    /// Run the initial input as it is.
    Root,
    /// Re-run `parent`, then find an input that flips condition `index` of its path.
    Flip { parent: ConcreteInput, index: usize },
}

/// Explores the paths on a rayon pool, sending the input found for each one.
struct Explorer {
    executor: ConcolicExecutor,
    method: Method,
    step_count: u32,
    deadline: Instant,
    /// Set when the consumer is gone.
    stop: Arc<AtomicBool>,
    tx: Sender<ConcreteInput>,
}

impl Explorer {
    /// Explores `item`, and spawns a task for each path found from it.
    fn visit<'s>(&'s self, scope: &rayon::Scope<'s>, item: WorkItem) {
        // If deadline has passed or the iterator has been dropped
        if self.stop.load(Relaxed) || Instant::now() >= self.deadline {
            return;
        }
        let Some((input, children)) = self.explore(item) else {
            return;
        };
        // The last task spawned runs first, so the children are explored in order.
        for child in children {
            scope.spawn(move |s| self.visit(s, child));
        }
        if self.tx.send(input).is_err() {
            self.stop.store(true, Relaxed);
        }
    }

    /// Returns the input on the path `item` describes, and the paths to explore from it,
    /// or `None` if no input follows that path.
    fn explore(&self, item: WorkItem) -> Option<(ConcreteInput, Vec<WorkItem>)> {
        // Made on every call: z3 terms belong to the thread that created them.
        let initial = self.executor.initial_input(&self.method).unwrap();
        let (input, depth) = match item {
            WorkItem::Root => (initial, 0),
            WorkItem::Flip { parent, index } => {
                let parent = from_concrete(&initial, &parent);
                let (_, path, _) = self.interpret(parent.clone());
                let solver = new_solver();
                let mut params = Params::new();
                params.set_u32("timeout", SOLVER_TIMEOUT_MS);
                solver.set_params(&params);
                for c in &path[..index] {
                    solver.assert(c);
                }
                solver.assert(path[index].not());
                if solver.check() != SatResult::Sat {
                    return None;
                }
                let model = solver.get_model().unwrap();
                (next_input(&model, &parent), index + 1)
            }
        };
        let (_, path, _) = self.interpret(input.clone());
        let input = to_concrete(&input);
        let children = (depth..path.len())
            .rev()
            .map(|index| WorkItem::Flip {
                parent: input.clone(),
                index,
            })
            .collect();
        Some((input, children))
    }

    fn interpret(&self, input: Input) -> (Vec<State>, Vec<Bool>, ExeResult) {
        self.executor
            .interpreter
            .interpret(&self.method, input, self.step_count)
    }
}

struct ConcolicPathsIter {
    /// Inputs found by the workers.
    rx: Receiver<ConcreteInput>,
    /// The initial input, with the symbolic terms of the consumer's thread.
    initial_input: Input,
    interpreter: Interpreter,
    method: Method,
    step_count: u32,
    deadline: Instant,
    stop: Arc<AtomicBool>,
    /// The thread running the explorer's pool.
    producer: Option<JoinHandle<()>>,
}

impl Iterator for ConcolicPathsIter {
    type Item = Run;

    fn next(&mut self) -> Option<Self::Item> {
        let timeout = self.deadline.saturating_duration_since(Instant::now());
        let concrete = match self.rx.recv_timeout(timeout) {
            Ok(concrete) => concrete,
            Err(RecvTimeoutError::Timeout) => return None,
            Err(RecvTimeoutError::Disconnected) => {
                // The explorer is done: either all paths are explored, or a task panicked.
                if let Some(producer) = self.producer.take()
                    && let Err(panic) = producer.join()
                {
                    std::panic::resume_unwind(panic);
                }
                return None;
            }
        };
        let input = from_concrete(&self.initial_input, &concrete);
        let (_, path, r) = self
            .interpreter
            .interpret(&self.method, input.clone(), self.step_count);
        Some((input, path, r))
    }
}

impl Drop for ConcolicPathsIter {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
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
