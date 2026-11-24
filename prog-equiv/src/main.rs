use std::{env, fs::File, io::BufReader, path::Path};

use eqbench::{
    case::CaseId,
    desc::{DESC_FILE, Desc},
};
use interpreter::{Class, ConcolicExecutor, ExeResult, FromJson, Interpreter, Method, next_input};
use z3::{Params, Solver, ast::Bool};

fn main() {
    let args: Vec<String> = env::args().collect();
    // Extra arguments (e.g. the method name `eqbench analyse` passes) are ignored:
    // the method is read from the case's `J-Desc.json`.
    if args.len() < 3 {
        eprintln!("usage: {} <old> <new>", args[0]);
        std::process::exit(2);
    }
    let class_old = class_from_path(&args[1], "oldV");
    let class_new = class_from_path(&args[2], "newV");

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
    if class_new.get_method(&name).is_none() {
        eprintln!("method `{name}` not found in {}", args[2]);
        std::process::exit(1);
    }
    let method: &Method = match class_old.get_method(&name) {
        Some(method) => method,
        None => {
            eprintln!("method `{name}` not found in {}", args[1]);
            std::process::exit(1);
        }
    };

    match are_equiv(class_old.clone(), class_new.clone(), method) {
        Some(true) => println!("eq"),
        Some(false) => println!("neq"),
        None => println!("unknown"),
    }
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
fn class_from_path(path: &str, main: &str) -> Class {
    let path = Path::new(path);
    let file_path = if path.is_dir() {
        let main_file = path.join(format!("{main}.json"));
        if main_file.is_file() {
            main_file
        } else {
            let jsons: Vec<_> = std::fs::read_dir(path)
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect();
            match jsons.as_slice() {
                [only] => only.clone(),
                _ => panic!(
                    "{}: no {main}.json and {} JSON files, multi-class programs are not supported",
                    path.display(),
                    jsons.len()
                ),
            }
        }
    } else {
        path.to_path_buf()
    };
    let file = File::open(&file_path).unwrap();
    let json = serde_json::from_reader(BufReader::new(file)).unwrap();
    Class::from_json(&json)
}

fn are_equiv(class_old: Class, class_new: Class, method: &Method) -> Option<bool> {
    let conc_old = ConcolicExecutor::new(class_old.clone());
    let conc_new = ConcolicExecutor::new(class_new.clone());
    let interpreter_new = Interpreter::new(class_new);
    let interpreter_old = Interpreter::new(class_old);

    let mut params = Params::new();
    params.set_u32("timeout", interpreter::SOLVER_TIMEOUT_MS);
    let res_old = conc_old.run(method, 1000, 20);
    let res_new = conc_new.run(method, 1000, 20);
    let mut is_unkown = false;
    for (old_input1, p1, r1) in res_old {
        for (old_input2, p2, r2) in &res_new {
            if r1 == *r2 {
                continue;
            }

            // for Ok results it's enough to check if the symbolic values are equal
            if let ExeResult::Ok(Some((ref o1, _))) = r1
                && let ExeResult::Ok(Some((o2, _))) = r2
                && o1.are_symbolic_equal(o2)
            {
                continue;
            }

            // If any of the traces are unfinished, we don't use them as evidence
            if matches!(r1, ExeResult::DidNotFinish) || matches!(r2, ExeResult::DidNotFinish) {
                continue;
            }

            // if for both cases we use the same concrete value we as inputs we can save some time
            let (ref params1, _) = old_input1;
            let (params2, _) = old_input2;
            let inputs_eq = params1.iter().zip(params2).fold(true, |acc, (p1, p2)| {
                // non-static methods they get the reference to the object calling as an input
                if p1.is_ref() && p2.is_ref() && p1 == p2 {
                    acc
                } else {
                    acc && p1.are_concrete_equal(p2)
                }
            });
            if inputs_eq {
                if let ExeResult::Ok(Some((ref o1, _))) = r1
                    && let ExeResult::Ok(Some((o2, _))) = r2
                    && o1.are_concrete_equal(o2)
                {
                    continue;
                } else {
                    // in all other cases the outputs are not equal since they were checked above
                    return Some(false);
                }
            }

            let s = Solver::new();
            s.set_params(&params);
            for c in &p1 {
                s.assert(c);
            }
            for c in p2 {
                s.assert(c);
            }
            let check_res = s.check();
            match check_res {
                // Sat: some input follows both paths and the results differ.
                z3::SatResult::Sat => {
                    let model = s.get_model().unwrap();
                    let input = next_input(&model, &old_input1);
                    let (_, _, old_res) = interpreter_old.interpret(method, input.clone(), 1000);
                    let (_, _, new_res) = interpreter_new.interpret(method, input.clone(), 1000);
                    if matches!(old_res, ExeResult::DidNotFinish)
                        || matches!(new_res, ExeResult::DidNotFinish)
                    {
                    } else if old_res != new_res
                        && let ExeResult::Ok(output1) = old_res
                        && let ExeResult::Ok(output2) = new_res
                    {
                        match (output1, output2) {
                            (None, None) => (),
                            (Some(o1), Some(o2)) => {
                                if !o1.0.are_concrete_equal(&o2.0) {
                                    eprintln!("{:?}", input);
                                    eprintln!("{:?}, {:?}", o1.0, o2.0);

                                    return Some(false);
                                }
                            }
                            _ => return Some(false),
                        }
                    }
                }
                z3::SatResult::Unknown => is_unkown = true,
                z3::SatResult::Unsat => {}
            }
            if let ExeResult::Ok(Some(out1)) = &r1
                && let ExeResult::Ok(Some(out2)) = r2
            {
                // TODO We need to put some thought into whether this is the correct equivalence of output (does reference change matter? prob no)
                let sym_out1 = out1.0.symbolic_values(&out1.1);
                let sym_out2 = out2.0.symbolic_values(&out2.1);
                if sym_out1.len() == sym_out2.len() {
                    let sym_bools: Box<_> = sym_out1
                        .iter()
                        .zip(sym_out2.iter())
                        .map(|(d1, d2)| d1.eq(d2))
                        .collect();
                    s.assert(Bool::and(&sym_bools).not());
                }
            }
            let check_res = s.check();
            match check_res {
                // Sat: some input follows both paths and the results differ.
                z3::SatResult::Sat => {
                    let model = s.get_model().unwrap();
                    let input = next_input(&model, &old_input1);
                    let (_, _, old_res) = interpreter_old.interpret(method, input.clone(), 1000);
                    let (_, _, new_res) = interpreter_new.interpret(method, input, 1000);
                    if old_res != new_res {
                        if let ExeResult::Ok(output1) = old_res
                            && let ExeResult::Ok(output2) = new_res
                        {
                            match (output1, output2) {
                                (None, None) => panic!(),
                                (Some(o1), Some(o2)) => {
                                    if !o1.0.are_concrete_equal(&o2.0) {
                                        eprintln!("{:?}, {:?}", o1.0, o2.0);
                                        return Some(false);
                                    } else {
                                        panic!()
                                    }
                                }
                                _ => return Some(false),
                            }
                        } else {
                            return Some(false);
                        }
                    } else {
                        panic!()
                    }
                }
                z3::SatResult::Unknown => is_unkown = true,
                z3::SatResult::Unsat => {}
            }
        }
    }
    if is_unkown { None } else { Some(true) }
}
