use std::path::Path;
use std::process::Command;

#[test]
fn tracked_sources_match_the_supported_host_policy() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(root.join("scripts/check-supported-host-surface.sh"))
        .current_dir(&root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
