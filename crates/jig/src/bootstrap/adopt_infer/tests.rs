use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

use super::super::git::git;
use super::*;
use crate::bootstrap::adopt_infer::scan::{MAX_SCAN_DEPTH, MAX_SCAN_WARNINGS};

mod diagnostics_and_ci;
mod ecosystem;
#[path = "tests/frontend.rs"]
mod frontend_tests;
mod rust_and_scan;

fn infer_sqlx(root: &Path, warnings: &mut Vec<String>) -> super::rust_sqlx::SqlxInference {
    let scan = RepoScan::collect(root, warnings);
    super::rust_sqlx::infer_sqlx(root, &scan, warnings)
}

fn infer_package_manager(root: &Path, warnings: &mut Vec<String>) -> Option<String> {
    let scan = RepoScan::collect(root, warnings);
    super::package_manager::infer_package_manager(root, &scan, warnings)
}

fn signal_values(value: &JsonValue) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["value"].as_str().unwrap())
        .collect()
}
