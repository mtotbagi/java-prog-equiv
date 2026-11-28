use std::{
    collections::HashSet,
    env,
    fs::File,
    io::BufReader,
    path::Path,
    time::{Duration, Instant},
};

use eqbench::{
    case::CaseId,
    desc::{DESC_FILE, Desc},
};
use interpreter::{
    Class, ExeResult, FromJson, Input, Interpreter, Method, Run, concolic_paths, next_input,
};
use z3::{
    DeclKind, SatResult, Solver,
    ast::{Ast, Bool},
};

fn main() {
    let deadline = deadline();
    let args: Vec<String> = env::args().collect();
    // Extra arguments (e.g. the method name `eqbench analyse` passes) are ignored:
    // the method is read from the case's `J-Desc.json`.
    if args.len() < 3 {
        eprintln!("usage: {} <old> <new>", args[0]);
        std::process::exit(2);
    }

    let verdict = run(&args, deadline);
    match verdict {
        Some(true) => println!("eq"),
        Some(false) => println!("neq"),
        None => println!("unknown"),
    }
}

fn run(args: &[String], deadline: Instant) -> Option<bool> {
    let (Ok(class_old), Ok(class_new)) = (
        class_from_path(&args[1], "oldV"),
        class_from_path(&args[2], "newV"),
    ) else {
        return None;
    };

    let desc = case_desc(&args[1]);
    // `program name` is `<class>.<entry>` when the changed method (`method name`)
    // is only called through an entry point, e.g. CLEVER's `Sub.main` calling `foo`:
    // there only the entry point is equivalent, so that is what is compared.
    let name = match desc.program.split_once('.') {
        Some((_, entry))
            if class_old.get_method(entry).is_some() && class_new.get_method(entry).is_some() =>
        {
            entry.to_string()
        }
        _ => desc.method,
    };
    for (class, dir) in [(&class_old, &args[1]), (&class_new, &args[2])] {
        if class.get_method(&name).is_none() {
            eprintln!("method `{name}` not found in {dir}");
            std::process::exit(1);
        }
    }
    let method = class_old.get_method(&name).unwrap().clone();

    are_equiv(class_old, class_new, &method, deadline)
}

/// Time budget when `EQBENCH_TIMEOUT` is not set.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
/// Time kept back from the budget to print the answer before being killed.
const TIMEOUT_MARGIN: Duration = Duration::from_secs(1);

/// When to stop searching: the timeout `eqbench analyse` gives in `EQBENCH_TIMEOUT`
/// (seconds), or `DEFAULT_TIMEOUT`, minus a margin.
fn deadline() -> Instant {
    let start = Instant::now();
    let timeout = env::var("EQBENCH_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(DEFAULT_TIMEOUT, Duration::from_secs);
    let budget = timeout.saturating_sub(TIMEOUT_MARGIN).max(timeout / 2);
    start + budget
}

/// Description of the case, read from its `J-Desc.json`.
///
/// `version_dir` is a directory given by `eqbench analyse`,
/// `target/eqbench/decompiled/<bench>-<prog>-<Eq|Neq>/<old|new>`, whose parent
/// names the case, described by
/// `eqbench/src_data/benchmarks/<bench>/<prog>/<Eq|Neq>/J-Desc.json`.
fn case_desc(version_dir: &str) -> Desc {
    let fail = |msg: String| -> ! {
        eprintln!("{msg}");
        std::process::exit(2);
    };
    let path = Path::new(version_dir);
    let id: CaseId = path
        .canonicalize()
        .ok()
        .and_then(|p| p.parent()?.file_name()?.to_str()?.parse().ok())
        .unwrap_or_else(|| {
            fail(format!(
                "{version_dir}: expected .../<bench>-<prog>-<Eq|Neq>/<old|new>"
            ))
        });
    let desc_file = eqbench::dataset_dir()
        .join(&id.bench)
        .join(&id.prog)
        .join(id.kind.as_str())
        .join(DESC_FILE);
    match Desc::load(&desc_file) {
        Ok(desc) => desc,
        Err(e) => fail(format!("{e:#}")),
    }
}

/// Loads a class from a JSON file, or from a directory (as given by `eqbench analyse`):
/// `<dir>/<main>.json`, or the only JSON file in the directory.
fn class_from_path(path: &str, main: &str) -> Result<Class, ()> {
    let path = Path::new(path);
    let file_path = if path.is_dir() {
        let jsons: Vec<_> = std::fs::read_dir(path)
            .map_err(|_| ())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        let main_file = path.join(format!("{main}.json"));
        match jsons.as_slice() {
            [_] if main_file.is_file() => main_file,
            [only] => only.clone(),
            _ => return Err(()),
        }
    } else {
        path.to_path_buf()
    };
    let file = File::open(&file_path).map_err(|_| ())?;
    let json = serde_json::from_reader(BufReader::new(file)).map_err(|_| ())?;
    Ok(Class::from_json(&json))
}

const DEFAULT_WORKERS: usize = 4;
const STEP_COUNT: u32 = 1000;

fn are_equiv(
    class_old: Class,
    class_new: Class,
    method: &Method,
    deadline: Instant,
) -> Option<bool> {
    let mut iter_old = concolic_paths(
        class_old.clone(),
        method,
        STEP_COUNT,
        deadline,
        DEFAULT_WORKERS,
    );
    let mut iter_new = concolic_paths(
        class_new.clone(),
        method,
        STEP_COUNT,
        deadline,
        DEFAULT_WORKERS,
    );
    let checker = Checker {
        old: Interpreter::new(class_old),
        new: Interpreter::new(class_new),
        method,
    };

    let mut old_runs: Vec<Run> = Vec::new();
    let mut new_runs: Vec<Run> = Vec::new();
    let mut unknown = false;

    while Instant::now() < deadline {
        let mut progressed = false;
        if let Some(old) = iter_old.next() {
            progressed = true;
            let pairs = new_runs.iter().map(|new| (&old, new));
            match checker.check_cases(pairs, deadline) {
                Some(true) => (),
                Some(false) => return Some(false),
                None => unknown = true,
            }
            old_runs.push(old);
        }
        if let Some(new) = iter_new.next() {
            progressed = true;
            let pairs = old_runs.iter().map(|old| (old, &new));
            match checker.check_cases(pairs, deadline) {
                Some(true) => (),
                Some(false) => return Some(false),
                None => unknown = true,
            }
            new_runs.push(new);
        }
        if !progressed {
            // Every path of both versions has been compared.
            break;
        }
    }
    // The time is up, and we haven't seen any neq cases, guess eq
    // Arguably we should return unkown, but then we could return unkown even if we hit a didnotfinish run
    if unknown { None } else { Some(true) }
}

/// Whether two results are concretely different.
fn results_differ(r1: &ExeResult, r2: &ExeResult) -> bool {
    r1 != r2
        && !matches!(
            (r1, r2),
            (ExeResult::Ok(Some((o1, _))), ExeResult::Ok(Some((o2, _)))) if o1.are_concrete_equal(o2)
        )
}

struct Checker<'a> {
    old: Interpreter,
    new: Interpreter,
    method: &'a Method,
}

impl Checker<'_> {
    fn check_cases<'b>(
        &self,
        mut cases: impl Iterator<Item = (&'b Run, &'b Run)>,
        deadline: Instant,
    ) -> Option<bool> {
        let mut is_unkown = false;
        while Instant::now() < deadline
            && let Some((a, b)) = cases.next()
        {
            match self.check_case(a, b) {
                Some(true) => (),
                Some(false) => return Some(false),
                None => is_unkown = true,
            }
        }
        if is_unkown { None } else { Some(true) }
    }

    fn check_case(&self, (input1, p1, r1): &Run, (input2, p2, r2): &Run) -> Option<bool> {
        if r1 == r2 {
            return Some(true);
        }

        // for Ok results it's enough to check if the symbolic values are equal
        if let (ExeResult::Ok(Some((o1, _))), ExeResult::Ok(Some((o2, _)))) = (r1, r2)
            && o1.are_symbolic_equal(o2)
        {
            return Some(true);
        }

        // If any of the traces are unfinished, we don't use them as evidence
        if matches!(r1, ExeResult::DidNotFinish) || matches!(r2, ExeResult::DidNotFinish) {
            return Some(true);
        }

        // if for both cases we use the same concrete value we as inputs we can save some time
        // non-static methods get the reference to the object calling as an input
        let same_input = input1
            .0
            .iter()
            .zip(&input2.0)
            .all(|(a, b)| (a.is_ref() && b.is_ref() && a == b) || a.are_concrete_equal(b));
        if same_input && results_differ(r1, r2) {
            return Some(false);
        }
        
        if trivially_disjoint(p1, p2) {
            // No input follows both paths.
            return Some(true);
        }

        let s = interpreter::new_solver();
        for c in p1.iter().chain(p2) {
            s.assert(c);
        }
        match s.check() {
            // No input follows both paths.
            SatResult::Unsat => return Some(true),
            SatResult::Unknown => return None,
            // Sat: try an input that follows both paths.
            SatResult::Sat => {
                let (old_res, new_res) = self.run_model(&s, input1);
                if matches!((&old_res, &new_res), (ExeResult::Ok(_), ExeResult::Ok(_)))
                    && results_differ(&old_res, &new_res)
                {
                    return Some(false);
                }
            }
        }
        if let (ExeResult::Ok(Some(out1)), ExeResult::Ok(Some(out2))) = (r1, r2) {
            let sym_out1 = out1.0.symbolic_values(&out1.1);
            let sym_out2 = out2.0.symbolic_values(&out2.1);
            if sym_out1.len() == sym_out2.len() {
                let sym_bools: Box<_> = sym_out1
                    .iter()
                    .zip(&sym_out2)
                    .map(|(d1, d2)| d1.eq(d2))
                    .collect();
                s.assert(Bool::and(&sym_bools).not());
            }
        }
        match s.check() {
            SatResult::Unsat => Some(true),
            SatResult::Unknown => None,
            // Sat: some input follows both paths and the results differ.
            SatResult::Sat => {
                let (old_res, new_res) = self.run_model(&s, input1);
                assert!(
                    results_differ(&old_res, &new_res),
                    "the solver found differing results, but the runs agree"
                );
                Some(false)
            }
        }
    }

    /// Runs both versions on the input the solver's model assigns.
    fn run_model(&self, solver: &Solver, input: &Input) -> (ExeResult, ExeResult) {
        let input = next_input(&solver.get_model().unwrap(), input);
        let (_, _, old) = self.old.interpret(self.method, input.clone(), STEP_COUNT);
        let (_, _, new) = self.new.interpret(self.method, input, STEP_COUNT);
        (old, new)
    }
}

/// `(not a)` -> (a, false); anything else -> (b, true).
fn literal(b: &Bool) -> (Bool, bool) {
    if b.safe_decl().is_ok_and(|d| d.kind() == DeclKind::Not) {
        (b.nth_child(0).unwrap().as_bool().unwrap(), false)
    } else {
        (b.clone(), true)
    }
}

/// Some constraint in `p1` is the syntactic negation of one in `p2`.
fn trivially_disjoint(p1: &[Bool], p2: &[Bool]) -> bool {
    let lits: HashSet<(Bool, bool)> = p1.iter().map(literal).collect();
    p2.iter()
        .map(literal)
        .any(|(atom, pol)| lits.contains(&(atom, !pol)))
}
