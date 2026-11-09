use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use eqbench::build::Layout;
use eqbench::case::{Case, Dataset, Kind};
use eqbench::diff::whole_file_diff;
use eqbench::term::Style;
use eqbench::{dataset_dir, output_dir};

/// Tooling around the EqBench dataset of equivalent and non-equivalent Java programs.
#[derive(Parser)]
#[command(name = "eqbench", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List every case as `<bench>-<prog>-<Eq|Neq>`
    List,
    /// Show the metadata of a case and the diff between its two versions
    Details {
        /// Case id, e.g. `caldat-caldat-Neq`
        id: String,
    },
    /// Compile every case to bytecode (javac) then to JSON (jvm2json) in `target/eqbench`
    Build,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<()> {
    let dataset = Dataset::load(&dataset_dir())?;
    match command {
        Command::List => list(&dataset),
        Command::Details { id } => details(&dataset, dataset.get(&id)?)?,
        Command::Build => build(&dataset)?,
    }
    Ok(())
}

fn build(dataset: &Dataset) -> Result<()> {
    let layout = Layout::new(output_dir());
    let report = eqbench::build::build(dataset, &layout)?;
    for failure in &report.compile_failures {
        eprintln!("failed to compile {}:\n{}", failure.what, failure.message);
    }
    for failure in &report.decompile_failures {
        eprintln!("failed to decompile {}:\n{}", failure.what, failure.message);
    }
    println!(
        "compiled {} sources ({} fixed, {} failures)",
        report.sources,
        report.fixed_sources,
        report.compile_failures.len(),
    );
    println!(
        "decompiled {} classes ({} failures)",
        report.classes,
        report.decompile_failures.len(),
    );
    println!("done in {:.1}s", report.elapsed.as_secs_f64());
    println!("output: {}", layout.root.display());
    Ok(())
}

fn list(dataset: &Dataset) {
    let width = dataset
        .cases
        .iter()
        .map(|c| c.id.to_string().len())
        .max()
        .unwrap_or(0);
    let method_width = dataset
        .cases
        .iter()
        .filter_map(|c| c.desc.as_ref().map(|d| d.method.len()))
        .max()
        .unwrap_or(0)
        .max("METHOD".len());

    println!(
        "{:width$}  {:4}  {:method_width$}  {:>4}  {:>7}",
        "ID", "KIND", "METHOD", "LOC", "CHANGES"
    );
    for case in &dataset.cases {
        let (method, loc, changes) = match &case.desc {
            Some(desc) => (
                desc.method.as_str(),
                desc.loc.to_string(),
                desc.changes.len().to_string(),
            ),
            None => ("-", "-".to_string(), "-".to_string()),
        };
        println!(
            "{:width$}  {:4}  {:method_width$}  {:>4}  {:>7}",
            case.id.to_string(),
            case.id.kind,
            method,
            loc,
            changes
        );
    }

    let eq = dataset
        .cases
        .iter()
        .filter(|c| c.id.kind == Kind::Eq)
        .count();
    println!(
        "\n{} cases ({} Eq, {} Neq)",
        dataset.cases.len(),
        eq,
        dataset.cases.len() - eq
    );
}

fn details(dataset: &Dataset, case: &Case) -> Result<()> {
    let style = Style::stdout();
    let verdict = match case.id.kind {
        Kind::Eq => "equivalent",
        Kind::Neq => "not equivalent",
    };
    println!("{} ({verdict})", style.bold(&case.id.to_string()));

    match &case.desc {
        None => println!("{}", style.dim("no J-Desc.json for this case")),
        Some(desc) => {
            println!("method           {}", desc.method);
            println!("LOC              {}", desc.loc);
            println!("loops            {}", desc.loops);
            println!("non-linear       {}", desc.non_linear);
            for (i, change) in desc.changes.iter().enumerate() {
                let label = if i == 0 { "changes" } else { "" };
                let lines: Vec<String> = change.lines.iter().map(|l| l.to_string()).collect();
                println!(
                    "{label:16} line {:8} {:7} {}",
                    lines.join(","),
                    change.kind.to_string(),
                    change.operation
                );
            }
            if let Some(params) = &desc.counter_example {
                for (i, param) in params.iter().enumerate() {
                    let label = if i == 0 { "counter-example" } else { "" };
                    println!("{label:16} {param}");
                }
            }
        }
    }
    println!();

    let old = fs::read_to_string(&case.old)
        .with_context(|| format!("cannot read {}", case.old.display()))?;
    let new = fs::read_to_string(&case.new)
        .with_context(|| format!("cannot read {}", case.new.display()))?;
    print!(
        "{}",
        whole_file_diff(
            &dataset.relative(&case.old).display().to_string(),
            &old,
            &dataset.relative(&case.new).display().to_string(),
            &new,
            style,
        )
    );
    Ok(())
}
