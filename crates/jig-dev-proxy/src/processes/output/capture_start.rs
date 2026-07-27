use std::process::Child;
use std::sync::{Arc, Mutex};

use super::{
    CancellableChildPipe, CaptureStartFailure, CapturedAppOutput, LiveAppProgress, OutputReader,
    TailBuffer, finish_output_readers, spawn_output_reader,
};
use crate::processes::child_lifecycle::terminate_and_reap_logged;

impl CapturedAppOutput {
    pub(in crate::processes) fn from_child(
        child: &mut Child,
        app_name: &str,
    ) -> std::result::Result<Self, CaptureStartFailure> {
        let Some(stdout) = child.stdout.take() else {
            return Err(capture_start_failure(
                child,
                None,
                Vec::new(),
                anyhow::anyhow!("Failed to capture development app stdout"),
            ));
        };
        let Some(stderr) = child.stderr.take() else {
            drop(stdout);
            return Err(capture_start_failure(
                child,
                None,
                Vec::new(),
                anyhow::anyhow!("Failed to capture development app stderr"),
            ));
        };
        let stdout = match CancellableChildPipe::new(stdout) {
            Ok(stdout) => stdout,
            Err(error) => {
                drop(stderr);
                return Err(capture_start_failure(
                    child,
                    None,
                    Vec::new(),
                    anyhow::Error::new(error)
                        .context("Failed to configure development app stdout capture"),
                ));
            }
        };
        let stderr = match CancellableChildPipe::new(stderr) {
            Ok(stderr) => stderr,
            Err(error) => {
                drop(stdout);
                return Err(capture_start_failure(
                    child,
                    None,
                    Vec::new(),
                    anyhow::Error::new(error)
                        .context("Failed to configure development app stderr capture"),
                ));
            }
        };
        let buffer = Arc::new(Mutex::new(TailBuffer::default()));
        let mut progress = match LiveAppProgress::new(app_name) {
            Ok(progress) => progress,
            Err(error) => {
                drop((stdout, stderr));
                return Err(capture_start_failure(
                    child,
                    None,
                    Vec::new(),
                    error.context("Failed to start development app progress renderer"),
                ));
            }
        };
        let stdout_reader =
            match spawn_output_reader(stdout, Arc::clone(&buffer), progress.state(), "stdout") {
                Ok(reader) => reader,
                Err(error) => {
                    drop(stderr);
                    return Err(capture_start_failure(
                        child,
                        Some(&mut progress),
                        Vec::new(),
                        anyhow::Error::new(error)
                            .context("Failed to start development app stdout capture"),
                    ));
                }
            };
        let stderr_reader =
            match spawn_output_reader(stderr, Arc::clone(&buffer), progress.state(), "stderr") {
                Ok(reader) => reader,
                Err(error) => {
                    return Err(capture_start_failure(
                        child,
                        Some(&mut progress),
                        vec![stdout_reader],
                        anyhow::Error::new(error)
                            .context("Failed to start development app stderr capture"),
                    ));
                }
            };
        Ok(Self {
            app_name: app_name.to_string(),
            buffer,
            readers: vec![stdout_reader, stderr_reader],
            progress,
            diagnostics: Vec::new(),
        })
    }
}

fn capture_start_failure(
    child: &mut Child,
    progress: Option<&mut LiveAppProgress>,
    mut readers: Vec<OutputReader>,
    error: anyhow::Error,
) -> CaptureStartFailure {
    let cleanup_confirmed = terminate_and_reap_logged(
        child,
        "could not clean up after output-capture startup failure",
    );
    if let Some(progress) = progress {
        let _ = progress.finish();
    }
    let mut diagnostics = Vec::new();
    finish_output_readers(&mut readers, &mut diagnostics);
    for diagnostic in diagnostics {
        eprintln!("Development app output capture startup was incomplete: {diagnostic}");
    }
    CaptureStartFailure {
        error,
        cleanup_confirmed,
    }
}
