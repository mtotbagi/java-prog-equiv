use std::{env, fs::File, io::BufReader, path::Path};

use eqbench::{
    case::CaseId,
    desc::{DESC_FILE, Desc},
};
use interpreter::{Class, ConcolicExecutor, ExeResult, FromJson, Method};
use z3::{Solver, ast::Bool};

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

    let equiv = are_equiv(class_old.clone(), class_new.clone(), method).unwrap_or(false);
    if equiv {
        println!("eq")
    } else {
        println!("neq")
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
    let conc_old = ConcolicExecutor::new(class_old);
    let conc_new = ConcolicExecutor::new(class_new);
    let res_old = conc_old.run(method, 1000, 20);
    let res_new = conc_new.run(method, 1000, 20);

    for (_, p1, r1) in res_old {
        for (_, p2, r2) in &res_new {
            if r1 == *r2 {
                continue;
            }
            let s = Solver::new();
            for c in &p1 {
                s.assert(c);
            }
            for c in p2 {
                s.assert(c);
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
                z3::SatResult::Sat => return Some(false),
                z3::SatResult::Unknown => return None,
                z3::SatResult::Unsat => {}
            }
        }
    }
    Some(true)
}
