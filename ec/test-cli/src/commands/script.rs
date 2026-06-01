//! CLI EC Test tool `script` subcommand: run text-DSL integration tests.
//!
//! SPDX-License-Identifier: MIT

use std::path::{Path, PathBuf};

use crate::cli::ScriptCommand;
use ec_test_lib::Source;
use ec_test_script::run_script;

pub fn run<S: Source>(source: S, cmd: ScriptCommand) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ScriptCommand::Run { path } => run_path(&source, &path),
    }
}

fn run_path<S: Source>(source: &S, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let meta = std::fs::metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    let files = if meta.is_dir() {
        collect_test_files(path)?
    } else {
        vec![path.to_path_buf()]
    };

    if files.is_empty() {
        return Err(format!("no `.test` files under {}", path.display()).into());
    }

    let mut total_passed = 0u32;
    let mut total_failed = 0u32;
    let mut failed_files: Vec<PathBuf> = Vec::new();
    let multi = files.len() > 1;

    for file in &files {
        if multi {
            println!("\n=== {} ===", file.display());
        }
        let script = std::fs::read_to_string(file).map_err(|e| format!("failed to read {}: {e}", file.display()))?;
        let summary = run_script(source, &script).map_err(|e| format!("{}: {e}", file.display()))?;
        println!(
            "[test] SUMMARY {}: {} passed, {} failed (total {})",
            file.display(),
            summary.passed,
            summary.failed,
            summary.total(),
        );
        total_passed += summary.passed;
        total_failed += summary.failed;
        if !summary.all_passed() {
            failed_files.push(file.clone());
        }
    }

    if multi {
        println!(
            "\n[test] TOTAL: {} passed, {} failed across {} file(s)",
            total_passed,
            total_failed,
            files.len(),
        );
        if !failed_files.is_empty() {
            println!("[test] failing files:");
            for f in &failed_files {
                println!("  - {}", f.display());
            }
        }
    }

    if total_failed > 0 {
        return Err(format!("{total_failed} test(s) failed").into());
    }
    Ok(())
}

/// Recursively collect every `*.test` file under `dir`, sorted for
/// reproducible run order.
fn collect_test_files(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    walk(dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk(&path, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|e| e == "test") {
            out.push(path);
        }
    }
    Ok(())
}
