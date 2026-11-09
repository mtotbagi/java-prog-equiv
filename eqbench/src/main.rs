use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

use eqbench::case::{Dataset, Kind};
use eqbench::dataset_dir;

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
    }
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
