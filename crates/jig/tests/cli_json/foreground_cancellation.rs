use super::*;

// Keep the lock held through the cleanup assertion: durable completion may wait
// for the writer, but supervision must remain able to stop the owned child.
#[test]
fn foreground_signal_cleans_up_child_while_run_journal_is_locked() {
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let temp = tempdir().unwrap();
        let marker = tempdir().unwrap();
        let pid_path = marker.path().join("child.pid");
        write_v6_failing_test_repo(temp.path());
        let config_path = temp.path().join(".jig.toml");
        let mut config: toml::Value =
            toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
        let quoted_path = pid_path.display().to_string().replace('\'', "'\\''");
        config["commands"]["api_test_command"] = toml::Value::String(format!(
            "printf '%s\\n' \"$$\" > '{quoted_path}'; exec sleep 60"
        ));
        fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
        let mut child = jig()
            .current_dir(temp.path())
            .args(["run", "api:test", "--no-receipt", "--json"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut target_pid = None;
        let started = wait_until(Duration::from_secs(15), || {
            target_pid = fs::read_to_string(&pid_path)
                .ok()
                .and_then(|value| value.trim().parse::<libc::pid_t>().ok())
                .filter(|pid| *pid > 0);
            target_pid.is_some()
        });
        if !started {
            let _ = child.kill();
            let _ = child.wait();
            panic!("foreground target never signalled readiness");
        }
        let target_pid = target_pid.expect("readiness requires a valid child PID");
        let journal = OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp.path().join(".agent/state/runs.jsonl"))
            .unwrap();
        FileExt::lock_exclusive(&journal).unwrap();
        // Allow a durable poll while holding the lock, then deliver the OS signal.
        std::thread::sleep(Duration::from_millis(250));
        let sent = unsafe { libc::kill(child.id() as libc::pid_t, signal) } == 0;
        let cleaned = wait_until(Duration::from_secs(10), || {
            (unsafe { libc::kill(target_pid, 0) }) == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        });
        FileExt::unlock(&journal).unwrap();
        let completed = wait_until(Duration::from_secs(10), || {
            child.try_wait().unwrap().is_some()
        });
        if !completed {
            let _ = child.kill();
        }
        let status = child.wait().unwrap();
        assert!(sent, "failed to deliver signal {signal}");
        assert!(
            cleaned,
            "signal {signal} did not clean up the child while journal was locked"
        );
        assert!(
            completed,
            "foreground run did not finish after journal unlock"
        );
        assert!(!status.success());
        let events = fs::read_to_string(temp.path().join(".agent/state/runs.jsonl")).unwrap();
        assert!(
            events.lines().any(|line| {
                let event: Value = serde_json::from_str(line).unwrap();
                event["event"] == "completed" && event["conclusion"] == "cancelled"
            }),
            "cancelled completion was not recorded: {events}"
        );
    }
}

fn wait_until(timeout: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
