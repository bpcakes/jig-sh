use std::cell::Cell;
use std::fs;
use std::io::Cursor;

use clap::Parser;
use tempfile::tempdir;

use super::*;
use crate::cli::{Cli, CommandKind};
use crate::test_env::lock_env;

fn init_opts(args: &[&str]) -> InitOpts {
    match Cli::try_parse_from(args).unwrap().command {
        CommandKind::Init(opts) => opts,
        other => panic!("expected init command, got {other:?}"),
    }
}

fn prepare(opts: &mut InitOpts) -> Result<bootstrap::PreparedInitAnswers> {
    prepare_init_interaction_with_io(opts, &mut Cursor::new(Vec::<u8>::new()), &mut Vec::new())
}

fn incompatible_error(input: &str) -> String {
    format!(
        "--preset rust-cli cannot be combined with incompatible input `{input}`; remove that input or select a matching preset"
    )
}

fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[path = "init_wizard_rust_cli_tests/acceptance.rs"]
mod acceptance;

#[path = "init_wizard_rust_cli_tests/rejections.rs"]
mod rejections;

#[path = "init_wizard_rust_cli_tests/prepared.rs"]
mod prepared;
