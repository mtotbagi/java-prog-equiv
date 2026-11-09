pub mod case;
pub mod desc;

use std::path::{Path, PathBuf};

/// Root of the crate, where the EqBench dataset is checked out.
fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The `benchmarks` directory of the EqBench dataset.
pub fn dataset_dir() -> PathBuf {
    crate_dir().join("src_data").join("benchmarks")
}
