use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use eqbench::analyse::{CaseResult, Outcome, Summary, TIMEOUT, run_case};
use eqbench::build::Layout;
use eqbench::case::{Case, Dataset, Kind};
use eqbench::diff::whole_file_diff;
use eqbench::term::Style;
use eqbench::{dataset_dir, output_dir};

/// `println!` returning an error instead of panicking when stdout is closed,
/// e.g. `eqbench list | head`.
macro_rules! outln {
    ($($arg:tt)*) => {
        writeln!(io::stdout(), $($arg)*)?
    };
}

macro_rules! out {
    ($($arg:tt)*) => {
        write!(io::stdout(), $($arg)*)?
    };
}

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
    /// Remove everything `build` produced
    Clean,
    /// Run an analysis tool on every case and count its correct predictions
    ///
    /// The tool is called as `<cmd…> <old-dir> <new-dir>`, the directories
    /// holding the JSON of every class of both versions, and must print `eq`
    /// or `neq` on the first line of its output.
    Analyse {
        /// The tool and its arguments
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) if is_broken_pipe(&err) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|e| {
        e.downcast_ref::<io::Error>()
            .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
    })
}

fn run(command: Command) -> Result<()> {
    let load = || Dataset::load(&dataset_dir());
    match command {
        Command::List => list(&load()?)?,
        Command::Details { id } => {
            let dataset = load()?;
            details(&dataset, dataset.get(&id)?)?;
        }
        Command::Build => build(&load()?)?,
        Command::Clean => clean()?,
        Command::Analyse { cmd } => analyse(&load()?, &cmd)?,
    }
    Ok(())
}

fn clean() -> Result<()> {
    let layout = Layout::new(output_dir());
    if eqbench::build::clean(&layout)? {
        outln!("removed {}", layout.root.display());
    } else {
        outln!("nothing to clean");
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
    outln!(
        "compiled {} sources ({} fixed, {} failures)",
        report.sources,
        report.fixed_sources,
        report.compile_failures.len(),
    );
    outln!(
        "decompiled {} classes ({} failures)",
        report.classes,
        report.decompile_failures.len(),
    );
    outln!("done in {:.1}s", report.elapsed.as_secs_f64());
    outln!("output: {}", layout.root.display());
    Ok(())
}

fn analyse(dataset: &Dataset, cmd: &[String]) -> Result<()> {
    let layout = Layout::new(output_dir());
    if !layout.root.join("decompiled").is_dir() {
        bail!("nothing is built, run `eqbench build` first");
    }
    let style = Style::stdout();
    let width = id_width(dataset);
    eprintln!(
        "analysing {} cases with `{}` (timeout {}s)",
        dataset.cases.len(),
        cmd.join(" "),
        TIMEOUT.as_secs()
    );

    let mut results: Vec<CaseResult> = Vec::with_capacity(dataset.cases.len());
    for case in &dataset.cases {
        let result = run_case(cmd, case, &layout)?;
        let mark = if result.is_correct() {
            style.green("✓")
        } else {
            style.red("✗")
        };
        let mut line = format!(
            "{:width$}  expected {:3}  got {:9}  {mark}  {:>9}",
            result.id.to_string(),
            Outcome::Predicted(result.id.kind).label(),
            result.outcome.label(),
            format_duration(result.elapsed),
        );
        if let Outcome::Invalid(output) = &result.outcome {
            line.push_str(&style.dim(&format!("  output: {output:?}")));
        }
        outln!("{line}");
        results.push(result);
    }

    let summary = Summary::new(&results);
    outln!();
    outln!(
        "{}",
        style.bold(&format!(
            "correct predictions: {}/{} ({:.1}%)",
            summary.correct,
            summary.total,
            summary.accuracy()
        ))
    );
    let slowest = summary
        .slowest
        .as_ref()
        .map(|(id, t)| format!(", max {} ({id})", format_duration(*t)))
        .unwrap_or_default();
    outln!(
        "time: total {}, mean {}{slowest}",
        format_duration(summary.total_time),
        format_duration(summary.mean_time()),
    );
    outln!(
        "errors: {} timeouts, {} crashes, {} invalid outputs, {} not built",
        summary.timeouts,
        summary.crashes,
        summary.invalid,
        summary.not_built
    );
    Ok(())
}

fn format_duration(d: Duration) -> String {
    if d < Duration::from_secs(1) {
        format!("{:.1} ms", d.as_secs_f64() * 1000.0)
    } else {
        format!("{:.2} s", d.as_secs_f64())
    }
}

fn id_width(dataset: &Dataset) -> usize {
    dataset
        .cases
        .iter()
        .map(|c| c.id.to_string().len())
        .max()
        .unwrap_or(0)
}

fn list(dataset: &Dataset) -> Result<()> {
    let width = id_width(dataset);
    let method_width = dataset
        .cases
        .iter()
        .filter_map(|c| c.desc.as_ref().map(|d| d.method.len()))
        .max()
        .unwrap_or(0)
        .max("METHOD".len());

    outln!(
        "{:width$}  {:4}  {:method_width$}  {:>4}  {:>7}",
        "ID",
        "KIND",
        "METHOD",
        "LOC",
        "CHANGES"
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
        outln!(
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
    outln!(
        "\n{} cases ({} Eq, {} Neq)",
        dataset.cases.len(),
        eq,
        dataset.cases.len() - eq
    );
    Ok(())
}

fn details(dataset: &Dataset, case: &Case) -> Result<()> {
    let style = Style::stdout();
    let verdict = match case.id.kind {
        Kind::Eq => "equivalent",
        Kind::Neq => "not equivalent",
    };
    outln!("{} ({verdict})", style.bold(&case.id.to_string()));

    match &case.desc {
        None => outln!("{}", style.dim("no J-Desc.json for this case")),
        Some(desc) => {
            outln!("method           {}", desc.method);
            outln!("LOC              {}", desc.loc);
            outln!("loops            {}", desc.loops);
            outln!("non-linear       {}", desc.non_linear);
            for (i, change) in desc.changes.iter().enumerate() {
                let label = if i == 0 { "changes" } else { "" };
                let lines: Vec<String> = change.lines.iter().map(|l| l.to_string()).collect();
                outln!(
                    "{label:16} line {:8} {:7} {}",
                    lines.join(","),
                    change.kind.to_string(),
                    change.operation
                );
            }
            if let Some(params) = &desc.counter_example {
                for (i, param) in params.iter().enumerate() {
                    let label = if i == 0 { "counter-example" } else { "" };
                    outln!("{label:16} {param}");
                }
            }
        }
    }
    outln!();

    let old = fs::read_to_string(&case.old)
        .with_context(|| format!("cannot read {}", case.old.display()))?;
    let new = fs::read_to_string(&case.new)
        .with_context(|| format!("cannot read {}", case.new.display()))?;
    out!(
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
