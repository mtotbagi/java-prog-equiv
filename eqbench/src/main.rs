use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use eqbench::analyse::{CaseResult, Outcome, Summary, TIMEOUT, resolve_tool, run_cases};
use eqbench::build::Layout;
use eqbench::case::{Case, CaseId, Dataset, Kind};
use eqbench::diff::whole_file_diff;
use eqbench::results::{self, Record, Run, Writer};
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
    /// Run an analysis tool on every case and score its predictions (accuracy, F1, time)
    ///
    /// The tool is called as `<cmd…> <old-dir> <new-dir> <method>`, the
    /// directories holding the source and the JSON of every class of both
    /// versions and `<method>` the method to compare, and must print `eq`,
    /// `neq` or `unknown` on the first line of its output. A directory holding
    /// a `flake.nix`, such as `eqbench/tools/pasda`, is built with nix first.
    Analyse(AnalyseArgs),
}

#[derive(clap::Args)]
struct AnalyseArgs {
    /// Seconds given to the tool for one case, also passed to it as `EQBENCH_TIMEOUT`
    #[arg(long, default_value_t = TIMEOUT.as_secs(), value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    /// Number of cases analysed at the same time
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
    jobs: u64,
    /// Write the result of every case to this file, as JSON lines
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Keep the results already in the output file and analyse the other cases only
    #[arg(long, requires = "output")]
    resume: bool,
    /// The tool and its arguments
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    cmd: Vec<String>,
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
        Command::Analyse(args) => analyse(&load()?, &args)?,
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

fn analyse(dataset: &Dataset, args: &AnalyseArgs) -> Result<()> {
    let (cmd, timeout, jobs) = (
        &args.cmd,
        Duration::from_secs(args.timeout),
        args.jobs as usize,
    );
    let layout = Layout::new(output_dir());
    if !layout.root.join("cases").is_dir() {
        bail!("nothing is built, run `eqbench build` first");
    }
    let tool = resolve_tool(cmd)?;
    let run = Run {
        tool: cmd.join(" "),
        timeout,
        jobs,
    };
    let (mut writer, mut done) = match &args.output {
        Some(path) => {
            let (writer, done) = open_results(path, args.resume, &run, dataset)?;
            (Some(writer), done)
        }
        None => (None, HashMap::new()),
    };
    let todo: Vec<&Case> = dataset
        .cases
        .iter()
        .filter(|c| !done.contains_key(&c.id))
        .collect();
    let style = Style::stdout();
    let width = id_width(dataset);
    let jobs_label = format!("{jobs} job{}", if jobs == 1 { "" } else { "s" });
    eprintln!(
        "analysing {} cases with `{}` (timeout {}s, {jobs_label})",
        todo.len(),
        cmd.join(" "),
        timeout.as_secs()
    );

    let start = Instant::now();
    let new = run_cases(&tool, &todo, &layout, timeout, jobs, |result| {
        let mark = if result.is_correct() {
            style.green("✓")
        } else if result.outcome == Outcome::Unknown {
            style.dim("?")
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
        if let Some(writer) = writer.as_mut() {
            writer.write(&Record::new(result, &run))?;
        }
        Ok(())
    })?;
    let wall = start.elapsed();
    done.extend(new.into_iter().map(|r| (r.id.clone(), r)));
    let results: Vec<CaseResult> = dataset
        .cases
        .iter()
        .filter_map(|c| done.remove(&c.id))
        .collect();

    let summary = Summary::new(&results);
    outln!();
    outln!(
        "{}",
        style.bold(&format!(
            "accuracy  {}/{} ({:.1}%)",
            summary.correct,
            summary.total,
            summary.accuracy()
        ))
    );
    outln!(
        "{}",
        style.bold(&format!(
            "f1        {:.3} (mean of eq and neq)",
            summary.macro_f1()
        ))
    );
    for kind in [Kind::Eq, Kind::Neq] {
        let score = summary.class(kind);
        outln!(
            "          {:3}  precision {:5}  recall {:5}  f1 {:.3}",
            Outcome::Predicted(kind).label(),
            format_ratio(score.precision()),
            format_ratio(score.recall()),
            score.f1()
        );
    }
    outln!("unknown   {}/{}", summary.unknown, summary.total);
    let slowest = summary
        .slowest
        .as_ref()
        .map(|(id, t)| format!(", max {} ({id})", format_duration(*t)))
        .unwrap_or_default();
    outln!(
        "time      total {}, mean {}{slowest}, wall {} with {jobs_label}",
        format_duration(summary.total_time),
        format_duration(summary.mean_time()),
        format_duration(wall),
    );
    outln!(
        "errors    {} timeouts, {} crashes, {} invalid outputs, {} not built",
        summary.timeouts,
        summary.crashes,
        summary.invalid,
        summary.not_built
    );
    Ok(())
}

/// Creates the results file of `run`. With `resume`, the records it already
/// holds are kept, and their results returned so that their cases are not
/// analysed again.
fn open_results(
    path: &Path,
    resume: bool,
    run: &Run,
    dataset: &Dataset,
) -> Result<(Writer, HashMap<CaseId, CaseResult>)> {
    let mut kept = Vec::new();
    if resume && path.exists() {
        for record in results::load(path)? {
            if record.tool != run.tool || record.timeout != run.timeout.as_secs() {
                bail!(
                    "{} holds results of `{}` with a timeout of {}s, not of this run",
                    path.display(),
                    record.tool,
                    record.timeout
                );
            }
            let result = record.to_result()?;
            if dataset.find(&result.id).is_some() {
                kept.push((record, result));
            }
        }
    }

    // Rewritten, without the truncated line that a killed run may leave.
    let mut writer = Writer::create(path)?;
    let mut done = HashMap::new();
    for (record, result) in kept {
        if !done.contains_key(&result.id) {
            writer.write(&record)?;
            done.insert(result.id.clone(), result);
        }
    }
    if !done.is_empty() {
        eprintln!(
            "resuming: {} cases already in {}",
            done.len(),
            path.display()
        );
    }
    Ok((writer, done))
}

/// A score in [0, 1] with 3 decimals, `-` when undefined.
fn format_ratio(ratio: Option<f64>) -> String {
    ratio.map_or_else(|| "-".to_string(), |r| format!("{r:.3}"))
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
        .map(|c| c.desc.entry().len())
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
        outln!(
            "{:width$}  {:4}  {:method_width$}  {:>4}  {:>7}",
            case.id.to_string(),
            case.id.kind,
            case.desc.entry(),
            case.desc.loc,
            case.desc.changes.len()
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

    let desc = &case.desc;
    if desc.entry() == desc.method {
        outln!("method           {}", desc.method);
    } else {
        outln!(
            "method           {} (changed: {})",
            desc.entry(),
            desc.method
        );
    }
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
