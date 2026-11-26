//! Implements a concolic executor as an iterator returning the possible paths a Java program can take.
//! Some paths (which cannot be solved by z3) will be discarded
//!
//! Internally uses rayon threadpool to preemtively compute items which returned by the iterator when needed
//!
//! As z3 is not thread-safe, no symbolic data is being passed between threads,
//! they are recomputed (cheaply) by interpreting the input they were generated from the first time.
use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering::Relaxed},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use z3::{
    Model, Params, SatResult, Solver, Sort,
    ast::{BV, Bool, Float, RoundingMode},
};

use crate::*;

pub type Input = (Vec<StackValue>, Heap);
pub type Run = (Input, Vec<Bool>, ExeResult);
const SOLVER_TIMEOUT_MS: u32 = 100;

pub fn new_solver() -> Solver {
    let solver = Solver::new_for_logic("QF_FPBV").unwrap();
    let mut params = Params::new();
    params.set_u32("timeout", SOLVER_TIMEOUT_MS);
    solver.set_params(&params);
    solver
}

/// Iterates over the paths `method` of `class` can take, each with an input following it.
pub fn concolic_paths(
    class: Class,
    method: &Method,
    step_count: u32,
    deadline: Instant,
    workers: usize,
) -> impl Iterator<Item = Run> {
    let runner = Runner {
        interpreter: Interpreter::new(class),
        method: method.clone(),
        step_count,
    };
    let initial_input = runner.initial_input();
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let explorer = Explorer {
        runner: runner.clone(),
        deadline,
        stop: stop.clone(),
        tx,
        queue: Mutex::default(),
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers.max(1))
        .build()
        .unwrap();
    let producer = thread::spawn(move || pool.scope(|s| explorer.push(s, 0, WorkItem::Root)));
    ConcolicPathsIter {
        rx,
        initial_input,
        runner,
        deadline,
        stop,
        producer: Some(producer),
    }
}

/// Runs one method of a class, shared by the explorer and the iterator.
#[derive(Clone)]
struct Runner {
    interpreter: Interpreter,
    method: Method,
    step_count: u32,
}

impl Runner {
    /// The input with every argument symbolic and zero.
    /// Made per thread: z3 terms belong to the thread that created them.
    fn initial_input(&self) -> Input {
        let mut args = Vec::new();
        let mut heap = Vec::new();
        if !self.method.is_static {
            args.push(StackValue::Ref(
                Some(0),
                Bool::from_bool(true),
                BV::from_u64(0, 32),
            ));
            heap.push(self.interpreter.class().new_object());
        }
        for (i, ty) in self.method.id.params.iter().enumerate() {
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
                    panic!("method argument type not implemented: {ty:?} (argument {i})")
                }
            };
            args.push(value);
        }
        (args, Heap { heap })
    }

    fn run(&self, input: Input) -> (Vec<Bool>, ExeResult) {
        let (_, path, result) = self
            .interpreter
            .interpret(&self.method, input, self.step_count);
        (path, result)
    }
}

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

/// A work item waiting in the queue. The highest `weight` is explored first.
struct Queued {
    weight: u64,
    item: WorkItem,
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}

impl Eq for Queued {}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> Ordering {
        self.weight.cmp(&other.weight)
    }
}

/// Explores the paths on a rayon pool, sending the input found for each one.
struct Explorer {
    runner: Runner,
    deadline: Instant,
    /// Set when the consumer is gone.
    stop: Arc<AtomicBool>,
    tx: Sender<ConcreteInput>,
    /// The items waiting to be explored, by priority.
    queue: Mutex<BinaryHeap<Queued>>,
}

impl Explorer {
    /// Queues `item`, and spawns a task to explore the highest priority queued item.
    ///
    /// Every push spawns exactly one task and every task pops exactly one item,
    /// so a task never finds the queue empty, and the scope ends when the queue does.
    fn push<'s>(&'s self, scope: &rayon::Scope<'s>, weight: u64, item: WorkItem) {
        self.queue.lock().unwrap().push(Queued { weight, item });
        scope.spawn(move |s| {
            let next = self.queue.lock().unwrap().pop().unwrap();
            self.visit(s, next.item);
        });
    }

    /// Explores `item`, and queues the paths found from it.
    fn visit<'s>(&'s self, scope: &rayon::Scope<'s>, item: WorkItem) {
        // If deadline has passed or the iterator has been dropped
        if self.stop.load(Relaxed) || Instant::now() >= self.deadline {
            return;
        }
        let Some((input, children)) = self.explore(item) else {
            return;
        };
        for (weight, child) in children {
            self.push(scope, weight, child);
        }
        if self.tx.send(input).is_err() {
            self.stop.store(true, Relaxed);
        }
    }

    /// Priority of flipping condition `index` of `path`: higher is explored first.
    fn weight(&self, _path: &[Bool], index: usize) -> u64 {
        u64::MAX - (index as u64 + 1)
    }

    /// Returns the input on the path `item` describes, and the weighted paths to explore
    /// from it, or `None` if no input follows that path.
    fn explore(&self, item: WorkItem) -> Option<(ConcreteInput, Vec<(u64, WorkItem)>)> {
        let initial = self.runner.initial_input();
        let (input, depth) = match item {
            WorkItem::Root => (initial, 0),
            WorkItem::Flip { parent, index } => {
                let parent = from_concrete(&initial, &parent);
                let (path, _) = self.runner.run(parent.clone());
                let model = flip(&path, index)?;
                (next_input(&model, &parent), index + 1)
            }
        };
        let (path, _) = self.runner.run(input.clone());
        let input = to_concrete(&input);
        let children = (depth..path.len())
            .rev()
            .map(|index| {
                let child = WorkItem::Flip {
                    parent: input.clone(),
                    index,
                };
                (self.weight(&path, index), child)
            })
            .collect();
        Some((input, children))
    }
}

fn flip(path: &[Bool], index: usize) -> Option<Model> {
    let solver = new_solver();
    for c in &path[..index] {
        solver.assert(c);
    }
    solver.assert(path[index].not());
    match solver.check() {
        SatResult::Sat => solver.get_model(),
        _ => None,
    }
}

struct ConcolicPathsIter {
    /// Inputs found by the workers.
    rx: Receiver<ConcreteInput>,
    /// The initial input, with the symbolic terms of the consumer's thread.
    initial_input: Input,
    runner: Runner,
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
        let (path, result) = self.runner.run(input.clone());
        Some((input, path, result))
    }
}

impl Drop for ConcolicPathsIter {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

/// The same input, but with the values the model assigns to the symbolic variables.
pub fn next_input(model: &Model, old_input: &Input) -> Input {
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
                // Widening f32 -> f64 is exact, so is the cast back.
                let solved = eval_double(model, &widen_to_double(f)).map_or(*old, |d| d as f32);
                StackValue::Float(solved, f.clone())
            }
            StackValue::Double(old, f) => {
                let solved = eval_double(model, f).unwrap_or(*old);
                StackValue::Double(solved, f.clone())
            }
            other => other.clone(),
        })
        .collect();
    (values, old_input.1.clone())
}

/// Value of a bitvector in the model, as unsigned bits.
fn eval_bits(model: &Model, bv: &BV) -> Option<u64> {
    model.eval(bv, false)?.as_u64()
}

/// Value of a double-sorted floating point term in the model.
fn eval_double(model: &Model, f: &Float) -> Option<f64> {
    Some(model.eval(f, false)?.as_f64())
}

/// Exact conversion of a single precision term to double precision.
/// Needed because z3's numeral -> double conversion flushes single precision
/// subnormals to zero.
fn widen_to_double(f: &Float) -> Float {
    f.to_fp_with_rounding_mode(&RoundingMode::round_nearest_ties_to_even(), &Sort::double())
}
