//! Mouse input reaching the interactive picker through a real terminal.

use super::*;

#[test]
fn interactive_picker_launches_a_double_clicked_home() {
    let temp = support::tempdir().unwrap();
    let default = temp.path().join(".codex");
    let work = temp.path().join(".codex-work");
    let launched = temp.path().join("launched-home");
    fs::create_dir(&default).unwrap();
    fs::create_dir(&work).unwrap();
    let stub = write_executable(
        temp.path().join("codex-stub.sh"),
        r#"#!/bin/sh
if [ "${1:-}" != "app-server" ]; then
  printf '%s\n' "$CODEX_HOME" > "$JIG_TEST_LAUNCHED"
  exit 0
fi
sleep 30
"#,
    );
    let Some((mut master, stdin, stdout)) = required_pseudo_terminal("picker mouse") else {
        return;
    };
    let mut child = ChildGuard::new(
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .args(["codex", "launch"])
            .env("HOME", temp.path())
            .env("CODEX_HOME", &default)
            .env("JIG_CODEX_BIN", &stub)
            .env("JIG_TEST_LAUNCHED", &launched)
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    set_nonblocking(&master);

    let mut output = Vec::new();
    for expected in ["\x1b[?1000h", "Codex Home Picker", "codex-work"] {
        read_until(&mut master, &mut output, expected, Duration::from_secs(3));
    }
    // At 120x30 two homes stack one per line below the column header: the
    // second home is on screen row 4, which SGR mouse reports count from 1.
    let click = b"\x1b[<0;11;5M\x1b[<0;11;5m";
    master.write_all(click).unwrap();
    master.write_all(click).unwrap();
    read_until(
        &mut master,
        &mut output,
        "\x1b[?1049l",
        Duration::from_secs(5),
    );
    let status =
        wait_for_child_while_draining(&mut child, &mut master, &mut output, Duration::from_secs(5))
            .unwrap_or_else(|| panic!("picker did not exit: {}", String::from_utf8_lossy(&output)));
    assert!(status.success(), "picker exited with {status}");
    assert!(
        String::from_utf8_lossy(&output).contains("\x1b[?1000l"),
        "mouse capture was not restored"
    );
    assert_eq!(
        fs::read_to_string(launched).unwrap().trim(),
        work.canonicalize().unwrap().to_string_lossy()
    );
}
