use super::tests::{config, fixture, git, prepare_focus, run_prepared, write};
use super::*;
use crate::state::{PlanOpenRequest, plans_open};

#[test]
fn automatic_focus_preserves_workspace_feature_owners() {
    let (temp, ctx) = fixture();
    let root = temp.path();
    for (directory, name) in [("selected", "example-selected"), ("other", "example-other")] {
        write(
            root,
            &format!("{directory}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[features]\nfoo = []\n"
            ),
        );
    }
    write(
        root,
        "selected/tests/sibling.rs",
        "#[test] fn sibling_passes() {}\n",
    );
    write(
        root,
        "other/src/lib.rs",
        "#[test] fn other_feature_is_enabled() { assert!(cfg!(feature = \"foo\")); }\n",
    );
    git(root, &["add", "."]);
    git(
        root,
        &["commit", "--quiet", "-m", "Example feature baseline"],
    );
    let opened = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Example feature context".into(),
            body: None,
            body_file: None,
            base: None,
        },
    )
    .unwrap();
    let focus = RustFocusV1::Automatic {
        plan_id: Some(opened["plan_id"].as_str().unwrap().into()),
    };
    write(
        root,
        "selected/src/lib.rs",
        "#[test] fn changed_library_passes() { assert_eq!(3 + 3, 6); }\n",
    );

    // A valid workspace policy must not become invalid just because automatic
    // impact excludes the package owning its qualified feature.
    let mut configured = config(true);
    configured.context.features = vec!["example-other/foo".into()];
    let broad = prepare_focus(&ctx, &configured, Some(focus.clone()));
    assert_eq!(broad.disposition, RustScopeDispositionV1::BroadFallback);
    assert!(broad.packages.is_empty());
    assert!(
        broad
            .reasons
            .iter()
            .any(|r| r == "feature_context_requires_workspace")
    );
    assert_eq!(broad.context, configured.context);
    let output = run_prepared(root, &broad);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("other_feature_is_enabled"));

    // Unqualified feature names have workspace-dependent meaning; preserve
    // workspace selection when metadata cannot prove their narrowed meaning.
    configured.context.features = vec!["foo".into()];
    let broad = prepare_focus(&ctx, &configured, Some(focus.clone()));
    assert_eq!(broad.disposition, RustScopeDispositionV1::BroadFallback);
    let output = run_prepared(root, &broad);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // An exact qualified feature owned by the selected package retains useful
    // narrowing, and the unchanged sibling's assertion must not be executed.
    configured.context.features = vec!["example-selected/foo".into()];
    let narrow = prepare_focus(&ctx, &configured, Some(focus));
    assert_eq!(narrow.disposition, RustScopeDispositionV1::Narrowed);
    assert_eq!(narrow.packages, ["example-selected@0.1.0"]);
    let output = run_prepared(root, &narrow);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("other_feature_is_enabled"));
}
