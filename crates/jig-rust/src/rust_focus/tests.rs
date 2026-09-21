use super::*;
use jig_contract::RustFeaturesV1;

fn config() -> RustNextestConfigV1 {
    RustNextestConfigV1 {
        workspace_manifest: "Cargo.toml".into(),
        focused: true,
        context: CargoImpactContextV1::default(),
        cargo_profile: None,
        nextest_profile: None,
    }
}

fn explicit(packages: &[&str], targets: Vec<RustTargetV1>) -> RustFocusV1 {
    RustFocusV1::Explicit {
        packages: packages.iter().map(|value| (*value).into()).collect(),
        targets,
        features: None,
        filter: None,
    }
}

#[test]
fn package_library_selection_is_build_scope_not_only_a_runtime_filter() {
    let config = config();
    let args = nextest_args(
        &config,
        &config.context,
        &["example-core@1.2.3".into()],
        &[RustTargetV1::Lib {}],
        None,
    );
    assert_eq!(
        args,
        [
            "nextest",
            "run",
            "--manifest-path",
            "Cargo.toml",
            "--no-tests=fail",
            "--profile",
            "default",
            "--package",
            "example-core@1.2.3",
            "--lib",
            "--locked",
            "--offline",
        ]
    );
    assert!(
        !args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--workspace" | "--all-targets"))
    );
}

#[test]
fn full_scope_has_no_implicit_package_or_test_filter() {
    let mut config = config();
    config.focused = false;
    assert_eq!(
        nextest_args(&config, &config.context, &[], &[], None),
        [
            "nextest",
            "run",
            "--manifest-path",
            "Cargo.toml",
            "--no-tests=fail",
            "--profile",
            "default",
            "--workspace",
            "--all-targets",
            "--locked",
            "--offline",
        ]
    );
}

#[test]
fn profiles_platform_features_and_filter_have_distinct_literal_argument_slots() {
    let mut config = config();
    config.workspace_manifest = "example workspace/Cargo.toml".into();
    config.cargo_profile = Some("release".into());
    config.nextest_profile = Some("ci".into());
    let context = CargoImpactContextV1 {
        target: Some("x86_64-unknown-linux-gnu".into()),
        features: vec!["example-core/serialization".into(), "logging".into()],
        no_default_features: true,
        ..Default::default()
    };
    let filter = "test(=example_case) | test(/[$();`\\ ]/ )";
    let args = nextest_args(
        &config,
        &context,
        &["example-core@1.2.3".into()],
        &[RustTargetV1::Test {
            name: "example_integration".into(),
        }],
        Some(filter),
    );
    assert_eq!(
        args,
        [
            "nextest",
            "run",
            "--manifest-path",
            "example workspace/Cargo.toml",
            "--no-tests=fail",
            "--package",
            "example-core@1.2.3",
            "--test",
            "example_integration",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--cargo-profile",
            "release",
            "--profile",
            "ci",
            "--features",
            "example-core/serialization",
            "--features",
            "logging",
            "--no-default-features",
            "--locked",
            "--offline",
            "--filter-expr",
            filter,
        ]
    );
}

#[test]
fn all_named_target_kinds_keep_their_exact_name_and_flag() {
    let config = config();
    for (target, flag) in [
        (
            RustTargetV1::Bin {
                name: "example".into(),
            },
            "--bin",
        ),
        (
            RustTargetV1::Test {
                name: "example".into(),
            },
            "--test",
        ),
        (
            RustTargetV1::Example {
                name: "example".into(),
            },
            "--example",
        ),
        (
            RustTargetV1::Bench {
                name: "example".into(),
            },
            "--bench",
        ),
    ] {
        let args = nextest_args(
            &config,
            &config.context,
            &["example@1.0.0".into()],
            &[target],
            None,
        );
        assert!(args.windows(2).any(|pair| pair == [flag, "example"]));
        assert!(!args.contains(&"--all-targets".into()));
    }
}

#[test]
fn focus_canonicalizes_repeated_packages_targets_and_features_idempotently() {
    let mut focus = explicit(
        &["example-z@1.0.0", "example-a@2.0.0", "example-z@1.0.0"],
        vec![
            RustTargetV1::Test { name: "z".into() },
            RustTargetV1::Lib {},
            RustTargetV1::Lib {},
        ],
    );
    if let RustFocusV1::Explicit { features, .. } = &mut focus {
        *features = Some(RustFeaturesV1 {
            features: vec!["z".into(), "a".into(), "z".into()],
            no_default_features: true,
            all_features: false,
        });
    }
    normalize_focus(&mut focus).unwrap();
    let RustFocusV1::Explicit {
        packages,
        targets,
        features,
        ..
    } = &focus
    else {
        panic!()
    };
    assert_eq!(packages, &["example-a@2.0.0", "example-z@1.0.0"]);
    assert_eq!(
        targets,
        &[
            RustTargetV1::Lib {},
            RustTargetV1::Test { name: "z".into() }
        ]
    );
    assert_eq!(features.as_ref().unwrap().features, ["a", "z"]);
    let once = focus.clone();
    normalize_focus(&mut focus).unwrap();
    assert_eq!(once, focus);
}

#[test]
fn selectors_reject_options_raw_ids_missing_identity_and_unbounded_tokens() {
    for selector in [
        "",
        "example",
        "@1.0.0",
        "example@",
        "--workspace@1.0.0",
        "example name@1.0.0",
        "example\n@1.0.0",
        "example\0@1.0.0",
        "example,other@1.0.0",
        "path+file:///tmp/example#example@1.0.0",
        "/tmp/example@1.0.0",
    ] {
        assert!(
            normalize_focus(&mut explicit(&[selector], vec![])).is_err(),
            "accepted {selector:?}"
        );
    }
    let too_long = format!("{}@1.0.0", "a".repeat(4097));
    assert!(normalize_focus(&mut explicit(&[&too_long], vec![])).is_err());
    assert!(normalize_focus(&mut explicit(&[], vec![])).is_err());
    assert!(normalize_focus(&mut explicit(&["example@1.0.0"; 33], vec![])).is_err());
    assert!(
        normalize_focus(&mut explicit(
            &["example@1.0.0"],
            vec![RustTargetV1::Lib {}; 33]
        ))
        .is_err()
    );
}

#[test]
fn target_names_and_automatic_plan_ids_reject_options_and_control_characters() {
    for name in ["", "--all-targets", "two names", "bad\nname", "bad\0name"] {
        assert!(
            normalize_focus(&mut explicit(
                &["example@1.0.0"],
                vec![RustTargetV1::Bin { name: name.into() }]
            ))
            .is_err()
        );
        assert!(
            normalize_focus(&mut RustFocusV1::Automatic {
                plan_id: Some(name.into())
            })
            .is_err()
        );
    }
    assert!(normalize_focus(&mut RustFocusV1::Automatic { plan_id: None }).is_ok());
}

#[test]
fn literal_filters_allow_shell_metacharacters_but_are_bounded_and_nul_free() {
    for (filter, valid) in [
        ("test(/[$();`]/)".to_owned(), true),
        ("a".repeat(4096), true),
        (String::new(), false),
        ("a".repeat(4097), false),
        ("test(example)\0".to_owned(), false),
    ] {
        let mut focus = explicit(&["example@1.0.0"], vec![]);
        if let RustFocusV1::Explicit { filter: value, .. } = &mut focus {
            *value = Some(filter.clone());
        }
        assert_eq!(
            normalize_focus(&mut focus).is_ok(),
            valid,
            "filter length {}",
            filter.len()
        );
        if valid {
            let config = config();
            let args = nextest_args(
                &config,
                &config.context,
                &["example@1.0.0".into()],
                &[],
                Some(&filter),
            );
            assert_eq!(
                &args[args.len() - 2..],
                &["--filter-expr".to_owned(), filter]
            );
        }
    }
}

#[test]
fn feature_policy_rejects_conflicts_and_preserves_independent_locked_offline_flags() {
    for target in [
        "/ExampleWorkspace/target.json",
        "C:\\ExampleWorkspace\\target.json",
        "targets/example.json",
    ] {
        assert!(
            validate_context(&CargoImpactContextV1 {
                target: Some(target.into()),
                ..Default::default()
            })
            .is_err()
        );
    }
    for (features, no_default_features) in [(vec!["example".into()], false), (vec![], true)] {
        let context = CargoImpactContextV1 {
            all_features: true,
            features,
            no_default_features,
            ..Default::default()
        };
        assert!(validate_context(&context).is_err());
        let mut focus = explicit(&["example@1.0.0"], vec![]);
        if let RustFocusV1::Explicit { features, .. } = &mut focus {
            *features = Some(RustFeaturesV1 {
                features: context.features.clone(),
                all_features: true,
                no_default_features,
            });
        }
        assert!(normalize_focus(&mut focus).is_err());
    }
    let config = config();
    let context = CargoImpactContextV1 {
        all_features: true,
        locked: false,
        offline: false,
        ..Default::default()
    };
    assert!(validate_context(&context).is_ok());
    let args = nextest_args(&config, &context, &[], &[], None);
    assert!(args.contains(&"--all-features".into()));
    assert!(!args.iter().any(|arg| matches!(
        arg.as_str(),
        "--locked" | "--offline" | "--no-default-features"
    )));
}

#[test]
fn feature_context_rejects_unsupported_format_options_and_resource_excess() {
    let invalid = [
        CargoImpactContextV1 {
            metadata_format_version: 2,
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["--all-features".into()],
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["a,b".into()],
            ..Default::default()
        },
        CargoImpactContextV1 {
            target: Some("--release".into()),
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["a".into(); 257],
            ..Default::default()
        },
        CargoImpactContextV1 {
            features: vec!["a".repeat(4096); 17],
            ..Default::default()
        },
    ];
    for context in invalid {
        assert!(validate_context(&context).is_err(), "accepted {context:?}");
    }
}

#[test]
fn lib_selection_includes_proc_macros_and_library_crate_types_not_other_targets() {
    for kind in ["lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"] {
        assert!(target_matches(
            &RustTargetV1::Lib {},
            "example_macros",
            &[kind.into()]
        ));
    }
    for kind in ["bin", "test", "example", "bench", "future-unknown-kind"] {
        assert!(!target_matches(
            &RustTargetV1::Lib {},
            "example",
            &[kind.into()]
        ));
    }
    let target = RustTargetV1::Test {
        name: "example".into(),
    };
    assert!(target_matches(&target, "example", &["test".into()]));
    assert!(!target_matches(&target, "other", &["test".into()]));
    assert!(!target_matches(&target, "example", &["bin".into()]));
}

#[test]
fn config_paths_and_profiles_cannot_escape_portable_argv_contract() {
    for path in [
        "",
        "/tmp/example/Cargo.toml",
        "../Cargo.toml",
        "./Cargo.toml",
        "example/../Cargo.toml",
        "example//Cargo.toml",
        "example\\Cargo.toml",
        "Cargo.toml\0",
        "manifest.json",
    ] {
        let mut config = config();
        config.workspace_manifest = path.into();
        assert!(validate_config(&config).is_err(), "accepted {path:?}");
    }
    for profile in ["", "--release", "two profiles", "ci\n"] {
        let mut config = config();
        config.cargo_profile = Some(profile.into());
        assert!(validate_config(&config).is_err());
        config.cargo_profile = None;
        config.nextest_profile = Some(profile.into());
        assert!(validate_config(&config).is_err());
    }
    let mut config = config();
    config.workspace_manifest = "example workspace/Cargo.toml".into();
    assert!(validate_config(&config).is_ok());
}
