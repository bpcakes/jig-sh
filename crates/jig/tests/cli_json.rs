mod support;

use std::fs;
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::os::fd::FromRawFd;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;

use serde_json::{Value, json};
use support::tempdir;

fn jig() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jig"));
    command
        .env_remove("JIG_REPO_ROOT")
        .env_remove("JIG_INVOKE_CWD")
        .env("NO_COLOR", "1");
    command
}

include!("cli_json/command_contracts.rs");
include!("cli_json/recovery_and_helpers.rs");
