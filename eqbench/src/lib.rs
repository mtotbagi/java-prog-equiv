pub mod build;
pub mod case;
pub mod desc;
pub mod diff;
pub mod term;

use std::path::{Path, PathBuf};

/// Root of the crate, where the EqBench dataset is checked out.
fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The `benchmarks` directory of the EqBench dataset.
pub fn dataset_dir() -> PathBuf {
    crate_dir().join("src_data").join("benchmarks")
}

/// Where the build artifacts go: `<workspace>/target/eqbench`.
pub fn output_dir() -> PathBuf {
    crate_dir()
        .parent()
        .unwrap_or_else(|| crate_dir())
        .join("target")
        .join("eqbench")
}
