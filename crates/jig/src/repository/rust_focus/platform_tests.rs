use std::{path::Path, process::Command};

use serde_json::Value;

use super::tests::{config, fixture, git, owned_command, prepare_focus, write};
use super::*;
use crate::state::{PlanOpenRequest, plans_open};

fn metadata(root: &Path, platform: Option<&str>) -> Value {
    let mut command = Command::new("cargo");
    command.args(["metadata", "--format-version", "1", "--locked", "--offline"]);
    if let Some(platform) = platform {
        command.args(["--filter-platform", platform]);
    }
    let output = owned_command(root, &mut command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn has_edge(metadata: &Value, owner: &str, dependency: &str) -> bool {
    let packages = metadata["packages"].as_array().unwrap();
    let id = |name: &str| {
        &packages
            .iter()
            .find(|package| package["name"] == name)
            .unwrap()["id"]
    };
    let node = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| &node["id"] == id(owner))
        .unwrap();
    node["deps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| &edge["pkg"] == id(dependency))
}

#[test]
fn automatic_cross_target_focus_preserves_host_proc_macro_consumers() {
    let (temp, ctx) = fixture();
    let root = temp.path();
    let (host_os, target) = if cfg!(target_os = "linux") {
        ("linux", "aarch64-apple-darwin")
    } else {
        ("macos", "x86_64-unknown-linux-gnu")
    };
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"selected\", \"other\", \"derive\", \"leaf\"]\nresolver = \"2\"\n",
    );
    write(
        root,
        "selected/Cargo.toml",
        "[package]\nname = \"example-selected\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nexample-derive = { path = \"../derive\" }\n",
    );
    write(
        root,
        "selected/src/lib.rs",
        "#[derive(example_derive::Example)]\npub struct ExampleType;\n",
    );
    write(
        root,
        "derive/Cargo.toml",
        &format!(
            "[package]\nname = \"example-derive\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[lib]\nproc-macro = true\n[target.'cfg(target_os = \"{host_os}\")'.dependencies]\nexample-leaf = {{ path = \"../leaf\" }}\n"
        ),
    );
    write(
        root,
        "derive/src/lib.rs",
        "#[proc_macro_derive(Example)]\npub fn example(_input: proc_macro::TokenStream) -> proc_macro::TokenStream {\n    let _value = example_leaf::VALUE;\n    proc_macro::TokenStream::new()\n}\n",
    );
    write(
        root,
        "leaf/Cargo.toml",
        "[package]\nname = \"example-leaf\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(root, "leaf/src/lib.rs", "pub const VALUE: u32 = 1;\n");
    let lock = owned_command(
        root,
        Command::new("cargo").args(["generate-lockfile", "--offline"]),
    );
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    git(root, &["add", "."]);
    git(
        root,
        &[
            "commit",
            "--quiet",
            "-m",
            "Example host proc macro baseline",
        ],
    );

    // These commands inspect real Cargo graphs only. The foreign target's
    // standard library need not be installed; no cross-compilation is performed.
    let unfiltered = metadata(root, None);
    let filtered = metadata(root, Some(target));
    for graph in [&unfiltered, &filtered] {
        assert!(has_edge(graph, "example-selected", "example-derive"));
    }
    assert!(has_edge(&unfiltered, "example-derive", "example-leaf"));
    assert!(!has_edge(&filtered, "example-derive", "example-leaf"));

    let opened = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Example cross-target host dependency".into(),
            body: None,
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let automatic = RustFocusV1::Automatic {
        plan_id: Some(opened["plan_id"].as_str().unwrap().into()),
    };
    write(root, "leaf/src/lib.rs", "pub const VALUE: u32 = 2;\n");

    // The unfiltered graph still proves the ordinary reverse-dependency
    // closure; this safeguard must not disable normal local iteration.
    let normal = prepare_focus(&ctx, &config(true), Some(automatic.clone()));
    assert_eq!(
        normal.disposition,
        RustScopeDispositionV1::Narrowed,
        "{normal:?}"
    );
    assert_eq!(
        normal.packages,
        [
            "example-derive@0.1.0",
            "example-leaf@0.1.0",
            "example-selected@0.1.0"
        ]
    );

    let mut configured = config(true);
    configured.context.target = Some(target.into());
    // Explicit scope is a caller selection, not a claim that this filtered
    // graph contains every affected host build unit.
    let explicit = prepare_focus(
        &ctx,
        &configured,
        Some(RustFocusV1::Explicit {
            packages: vec!["example-selected@0.1.0".into()],
            targets: vec![jig_contract::RustTargetV1::Lib {}],
            features: None,
            filter: None,
        }),
    );
    assert_eq!(explicit.disposition, RustScopeDispositionV1::Narrowed);
    assert_eq!(explicit.packages, ["example-selected@0.1.0"]);
    assert!(
        explicit
            .args
            .windows(2)
            .any(|args| args == ["--target", target])
    );

    let automatic = prepare_focus(&ctx, &configured, Some(automatic));
    assert_eq!(
        automatic.disposition,
        RustScopeDispositionV1::BroadFallback,
        "filtered graph omitted a host dependency: {automatic:?}"
    );
    assert!(automatic.packages.is_empty());
    assert!(automatic.args.iter().any(|arg| arg == "--workspace"));
    assert!(
        automatic
            .args
            .windows(2)
            .any(|args| args == ["--target", target])
    );
    assert_eq!(automatic.context, configured.context);
    assert!(
        automatic
            .reasons
            .iter()
            .any(|reason| reason.contains("UnsupportedContext")),
        "{:?}",
        automatic.reasons
    );
    assert!(
        !root.join("target").exists(),
        "metadata-only regression must not claim cross-compilation"
    );
}
