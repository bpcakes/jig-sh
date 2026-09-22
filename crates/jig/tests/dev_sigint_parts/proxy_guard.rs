struct ProxyRuntimeGuard {
    repo: PathBuf,
    state_dir: PathBuf,
    http_port: u16,
    armed: bool,
}

struct ProxyStopOutput {
    output: std::process::Output,
    repo: PathBuf,
    state_dir: PathBuf,
    http_port: u16,
    state_entries: String,
    identity_verified_fallback_stopped_proxy: bool,
}

impl ProxyStopOutput {
    fn success(&self) -> bool {
        self.output.status.success() || self.identity_verified_fallback_stopped_proxy
    }
}

impl std::fmt::Display for ProxyStopOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}\nrepo: {}\nstate dir: {}\nHTTP port: {}\nstate entries: {}\nidentity-verified fallback stopped proxy: {}\nstdout:\n{}\nstderr:\n{}",
            self.output.status,
            self.repo.display(),
            self.state_dir.display(),
            self.http_port,
            self.state_entries,
            self.identity_verified_fallback_stopped_proxy,
            String::from_utf8_lossy(&self.output.stdout),
            String::from_utf8_lossy(&self.output.stderr),
        )
    }
}

impl ProxyRuntimeGuard {
    fn new(repo: &Path, state_dir: &Path, http_port: u16) -> Self {
        Self {
            repo: repo.to_path_buf(),
            state_dir: state_dir.to_path_buf(),
            http_port,
            armed: true,
        }
    }

    fn stop(mut self) -> ProxyStopOutput {
        self.armed = false;
        let proxy_identity = capture_test_proxy_identity(&self.state_dir);
        let output = self.stop_inner().expect("stop background proxy");
        // These signal tests exercise foreground-session teardown, not proxy
        // service-manager discovery. `proxy stop` intentionally fails closed
        // when a CI host has no inspectable user service manager, so retain its
        // output but clean up only the exact test-owned proxy identity.
        let identity_verified_fallback_stopped_proxy = if output.status.success() {
            false
        } else if let Some(proxy_identity) = proxy_identity.as_ref() {
            terminate_verified_process(proxy_identity);
            !proxy_identity.is_live()
        } else {
            false
        };
        ProxyStopOutput {
            output,
            repo: self.repo.clone(),
            state_dir: self.state_dir.clone(),
            http_port: self.http_port,
            state_entries: state_dir_entries(&self.state_dir),
            identity_verified_fallback_stopped_proxy,
        }
    }

    fn stop_inner(&self) -> std::io::Result<std::process::Output> {
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .args(["proxy", "stop", "--state-dir"])
            .arg(&self.state_dir)
            .args(["--http-port", &self.http_port.to_string()])
            .current_dir(&self.repo)
            .env_remove("JIG_REPO_ROOT")
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
    }
}

#[cfg(target_os = "linux")]
fn capture_test_proxy_identity(state_dir: &Path) -> Option<VerifiedProcessIdentity> {
    let pid = fs::read_to_string(state_dir.join("proxy.pid"))
        .ok()?
        .trim()
        .parse::<libc::pid_t>()
        .ok()?;
    let identity = VerifiedProcessIdentity::capture(pid)?;
    let process_exe = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let expected_exe = fs::canonicalize(env!("CARGO_BIN_EXE_jig")).ok()?;
    (process_exe == expected_exe).then_some(identity)
}

#[cfg(target_os = "macos")]
fn capture_test_proxy_identity(_state_dir: &Path) -> Option<VerifiedProcessIdentity> {
    // The CI failure this fallback addresses is Linux systemd user-manager
    // discovery. Keep macOS cleanup on the public stop path until an equally
    // strong executable-identity check is available there.
    None
}

fn state_dir_entries(state_dir: &Path) -> String {
    match fs::read_dir(state_dir) {
        Ok(entries) => {
            let mut names = entries
                .map(|entry| match entry {
                    Ok(entry) => entry.file_name().to_string_lossy().into_owned(),
                    Err(error) => format!("<entry error: {error}>"),
                })
                .collect::<Vec<_>>();
            names.sort();
            format!("{names:?}")
        }
        Err(error) => format!("<unavailable: {error}>"),
    }
}

impl Drop for ProxyRuntimeGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.stop_inner();
        }
    }
}
