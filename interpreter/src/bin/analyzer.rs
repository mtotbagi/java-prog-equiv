use std::{env, fmt::Display};

use interpreter::*;
use serde_json::Value;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args[1] == "info" {
        println!("Rust analyzer (not that one)\n0.1.0\nGroup Rust\nrust,dynamic\n");
        return;
    }
    let (classname, methodname) = get_class_and_method(&args[1]);
    let json: Value = read_json(&classname);
    let class = Class::from_json(&json);

    let method = class
        .get_method(&methodname)
        .unwrap_or_else(|| {
            panic!(
                "Method {} should be implemented on {}",
                methodname, classname
            )
        })
        .clone();
    if method
        .id
        .params
        .iter()
        .any(|param| matches!(param, SimpleType::SimpleRef(_) | SimpleType::Boolean))
    {
        println!("ok;skip");
        println!("divide by zero;skip");
        println!("assertion error;skip");
        println!("out of bounds;skip");
        println!("null pointer;skip");
        println!("*;skip");
        return;
    }

    let mut pred = Prediction {
        ok: "no".to_string(),
        div0: "no".to_string(),
        assert_err: "no".to_string(),
        out_of_bounds: "no".to_string(),
        null_pointer: "no".to_string(),
        no_halt: "no".to_string(),
    };

    let executor = ConcolicExecutor::new(class);
    let res = executor.run(&method, 1000, 100);
    use std::collections::HashSet;
    use std::mem::discriminant;
    dbg!(res.clone());

    let mut seen = HashSet::new();
    let unique: Vec<ExeResult> = res
        .into_iter()
        .map(|(_, _, exe_res)| exe_res)
        .filter(|x| seen.insert(discriminant(x)))
        .collect();
    for res in unique {
        match res {
            ExeResult::Ok(_) => pred.ok = "yes".to_string(),
            ExeResult::AssertErr => pred.assert_err = "yes".to_string(),
            ExeResult::OutOfBounds => pred.out_of_bounds = "yes".to_string(),
            ExeResult::NullPointer => pred.null_pointer = "yes".to_string(),
            ExeResult::Div0 => pred.div0 = "yes".to_string(),
            ExeResult::NoHalt => pred.no_halt = "yes".to_string(),
            ExeResult::DidNotFinish => {
                if pred.no_halt != "yes" {
                    pred.no_halt = "probably".to_string()
                }
            }
        }
    }
    println!("{}", pred)
}

struct Prediction {
    ok: String,
    div0: String,
    assert_err: String,
    out_of_bounds: String,
    null_pointer: String,
    no_halt: String,
}

impl Display for Prediction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "ok;{}", self.ok)?;
        writeln!(f, "divide by zero;{}", self.div0)?;
        writeln!(f, "assertion error;{}", self.assert_err)?;
        writeln!(f, "out of bounds;{}", self.out_of_bounds)?;
        writeln!(f, "null pointer;{}", self.null_pointer)?;
        write!(f, "*;{}", self.no_halt)?;
        Ok(())
    }
}
