use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use jig_ui::dashboard::{
    AcceptedProviderReport, BoundUnit, BoundedRows, BoundedText, CollectionDomain, LIMIT_SPECS,
    LimitError, LimitId, LimitShape, PARITY_REGISTRY, PLAN_ROOT_FIELDS, ProviderReportError,
    RECORDER_ROOT_FIELDS, ROOT_LIMIT_KEYS, RecorderEpochId, SNAPSHOT_ERROR_CODES,
    SNAPSHOT_ERROR_SCOPES, STATUS_ROOT_FIELDS, SnapshotError, SnapshotErrorCode, SourceError,
    TimelineLimit, root_limit, scenarios, validate_input_bytes,
};
use serde_json::{Value, json};

#[path = "dashboard_contract/parity_resolver.rs"]
mod parity_resolver;

#[test]
fn recorder_schema_one_matches_checked_in_golden() {
    let actual = serde_json::to_value(scenarios::recorder_snapshot()).unwrap();
    let expected: Value = serde_json::from_str(include_str!("fixtures/recorder-v1.json")).unwrap();
    assert_eq!(actual, expected);
    assert_root_fields(&actual, RECORDER_ROOT_FIELDS);
    assert_eq!(actual["command"], "ui");
    assert_eq!(actual["schema_version"], 1);
    assert_eq!(actual["snapshot_kind"], "recorder");
    assert!(actual["harness"]["jig_version"].is_null());
    assert!(actual["errors"].as_array().unwrap().is_empty());
}

#[test]
fn plan_schema_one_matches_checked_in_golden() {
    let actual = serde_json::to_value(scenarios::plan_snapshot()).unwrap();
    let expected: Value = serde_json::from_str(include_str!("fixtures/plan-v1.json")).unwrap();
    assert_eq!(actual, expected);
    assert_root_fields(&actual, PLAN_ROOT_FIELDS);
    assert_eq!(actual["command"], "ui");
    assert_eq!(actual["schema_version"], 1);
    assert_eq!(actual["snapshot_kind"], "plan");
    assert!(actual["plan"]["closed_at_ms"].is_null());
    assert!(actual["errors"].as_array().unwrap().is_empty());
}

#[test]
fn versioned_snapshots_round_trip_without_contract_loss() {
    let recorder = serde_json::to_value(scenarios::recorder_snapshot()).unwrap();
    let decoded: jig_ui::dashboard::RecorderSnapshot =
        serde_json::from_value(recorder.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), recorder);

    let plan = serde_json::to_value(scenarios::plan_snapshot()).unwrap();
    let decoded: jig_ui::dashboard::PlanSnapshot = serde_json::from_value(plan.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), plan);

    let status = serde_json::to_value(scenarios::status_snapshot()).unwrap();
    let decoded: jig_ui::dashboard::StatusSnapshot =
        serde_json::from_value(status.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), status);
}

#[test]
fn status_contract_has_the_existing_schema_one_root() {
    let actual = serde_json::to_value(scenarios::status_snapshot()).unwrap();
    let expected: Value = serde_json::from_str(include_str!("fixtures/status-v1.json")).unwrap();
    assert_eq!(actual, expected);
    assert_root_fields(&actual, STATUS_ROOT_FIELDS);
    assert_eq!(actual["command"], "status");
    assert_eq!(actual["schema_version"], 1);
    assert_eq!(actual["outcome"], "complete");
    assert!(actual["work"]["state"]["open_plans"][0]["baseline"].is_object());
    assert!(actual["work"]["gates"][0]["snapshot"]["gates"].is_array());
    assert!(actual["loops"]["attempts"].is_array());
    assert!(actual["loops"]["needs_attention"]["exhausted_attempts"].is_array());
    assert!(
        actual["loops"]["needs_attention"]["exhausted_attempts"][0]
            .get("remediation")
            .is_none()
    );
}

#[test]
fn status_gate_wire_shapes_preserve_legacy_target_and_optional_reason() {
    let evidence = json!({
        "kind": "evidence",
        "id": "api-evidence",
        "required": true,
        "target": "api:test",
        "profile": null,
        "conclusion": "success",
        "status": "passed",
        "run_id": null,
        "freshness": "fresh",
        "freshness_reason": "current",
        "targets": []
    });
    let decoded: jig_ui::dashboard::StatusGate = serde_json::from_value(evidence.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), evidence);

    let unsupported = json!({
        "kind": "future_gate",
        "id": "future",
        "required": false,
        "status": "unsupported",
        "future": 7
    });
    let decoded: jig_ui::dashboard::StatusGate =
        serde_json::from_value(unsupported.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), unsupported);
}

#[test]
fn accepted_provider_report_serializes_the_exact_raw_document_only() {
    let raw = scenarios::provider_raw_report();
    let accepted = AcceptedProviderReport::from_raw(raw.clone()).unwrap();

    assert_eq!(accepted.decoded().provider.id, "example-provider");
    assert_eq!(
        accepted.decoded().extensions["example.root"]["preserved"],
        true
    );
    assert_eq!(
        accepted.decoded().provider.extensions["example.identity"]["preserved"],
        true
    );
    assert_eq!(
        accepted.decoded().work_packages[0].extensions["example.package"]["preserved"],
        true
    );
    assert_eq!(accepted.raw(), &raw);
    assert_eq!(serde_json::to_value(&accepted).unwrap(), raw);

    let round_trip: AcceptedProviderReport = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(serde_json::to_value(round_trip).unwrap(), raw);
}

#[test]
fn accepted_provider_report_rejects_semantically_invalid_input() {
    let mut raw = scenarios::provider_raw_report();
    raw["provider"]["id"] = Value::String(String::new());
    assert!(matches!(
        AcceptedProviderReport::from_raw(raw),
        Err(ProviderReportError::Validation(_))
    ));
}

#[test]
fn raw_identity_controls_selection_when_display_text_collides() {
    let (left, right) = scenarios::colliding_identities();
    assert_eq!(left.display(), right.display());
    assert_ne!(left, right);

    let aliases = [
        left.clone(),
        jig_ui::dashboard::SelectableIdentity::new(left.raw(), "different display"),
    ];
    assert_eq!(aliases[0], aliases[1]);

    let identities = HashSet::from([left, right]);
    assert_eq!(identities.len(), 2);

    let ordered = identities.into_iter().collect::<BTreeSet<_>>();
    assert_eq!(ordered.first().unwrap().raw(), "provider\u{1b}[31mA");
    assert_eq!(ordered.last().unwrap().raw(), "provider\u{202e}A");
}

#[test]
fn unsupported_status_gate_kinds_round_trip_without_loss() {
    for (wire, expected) in [
        (
            serde_json::json!({
            "kind": "future_gate",
            "id": "future",
            "required": true,
            "status": "unsupported",
            "reason": "new producer kind",
            "future_policy": {"mode": "strict"}
            }),
            serde_json::json!({
                "kind": "future_gate",
                "id": "future",
                "required": true,
                "status": "unsupported",
                "reason": "new producer kind",
                "future_policy": {"mode": "strict"}
            }),
        ),
        (
            serde_json::json!({
                "kind": "check",
                "id": "future-check-shape",
                "required": false,
                "status": "unsupported",
                "reason": null
            }),
            serde_json::json!({
                "kind": "check",
                "id": "future-check-shape",
                "required": false,
                "status": "unsupported"
            }),
        ),
    ] {
        let decoded: jig_ui::dashboard::StatusGate = serde_json::from_value(wire).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }
}

#[test]
fn every_limit_identifier_and_ceiling_matches_the_plan() {
    let actual = LIMIT_SPECS
        .iter()
        .map(|spec| {
            (
                spec.id.as_str(),
                spec.ceiling,
                spec.shape,
                spec.serialized_at_root,
            )
        })
        .collect::<Vec<_>>();
    let expected = vec![
        ("open_plans", 1_000, LimitShape::RootRows, true),
        ("history", 10, LimitShape::RootRows, true),
        ("failures", 10, LimitShape::RootRows, true),
        ("failure_stderr_chars", 400, LimitShape::NestedText, false),
        ("tool_stats", 256, LimitShape::RootRows, true),
        ("loop_workflows", 1_000, LimitShape::NestedRows, false),
        ("loop_leases", 1_000, LimitShape::NestedRows, false),
        ("loop_attempts", 1_000, LimitShape::NestedRows, false),
        (
            "loop_scheduled_occurrences",
            1_000,
            LimitShape::NestedRows,
            false,
        ),
        (
            "loop_waiting_attempts",
            1_000,
            LimitShape::NestedRows,
            false,
        ),
        (
            "loop_exhausted_attempts",
            1_000,
            LimitShape::NestedRows,
            false,
        ),
        ("timeline", 1_000, LimitShape::RootRows, true),
        (
            "timeline_decision_rationale_chars",
            300,
            LimitShape::NestedText,
            false,
        ),
        ("gate_rows", 256, LimitShape::NestedRows, false),
        ("gate_changed_paths", 100, LimitShape::NestedRows, false),
        ("gate_matching_paths", 100, LimitShape::NestedRows, false),
        ("gate_findings", 100, LimitShape::NestedRows, false),
        ("plan_body_chars", 20_000, LimitShape::NestedText, false),
        (
            "plan_body_input_bytes",
            80_004,
            LimitShape::InputBytes,
            false,
        ),
        ("plan_decisions", 100, LimitShape::RootRows, true),
        ("plan_receipts", 50, LimitShape::RootRows, true),
        ("receipt_changed_paths", 20, LimitShape::NestedRows, false),
        ("receipt_stdout_chars", 1_000, LimitShape::NestedText, false),
        ("receipt_stderr_chars", 1_000, LimitShape::NestedText, false),
    ];
    assert_eq!(actual, expected);

    let unique = LIMIT_SPECS
        .iter()
        .map(|spec| spec.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), LIMIT_SPECS.len());
    assert_eq!(
        LIMIT_SPECS
            .iter()
            .filter(|spec| spec.serialized_at_root)
            .map(|spec| spec.id.as_str())
            .collect::<BTreeSet<_>>(),
        ROOT_LIMIT_KEYS.iter().copied().collect()
    );
}

#[test]
fn bounded_values_reject_over_limit_payloads() {
    assert!(BoundedRows::from_total(vec![1], 1, Some(1)).is_ok());
    let rows_error = BoundedRows::from_total(vec![1, 2], 1, Some(2)).unwrap_err();
    assert_eq!(rows_error.retained, 2);
    assert_eq!(rows_error.applied, 1);
    assert_eq!(rows_error.unit, BoundUnit::Rows);

    assert!(BoundedText::from_total("é", 1, Some(1)).is_ok());
    let text_error = BoundedText::from_total("éx", 1, Some(2)).unwrap_err();
    assert_eq!(text_error.retained, 2);
    assert_eq!(text_error.applied, 1);
    assert_eq!(text_error.unit, BoundUnit::Characters);
}

#[test]
fn bounded_values_derive_omissions_and_reject_malformed_wire_data() {
    let rows = BoundedRows::from_total(vec![1], 1, Some(2)).unwrap();
    assert_eq!(rows.items(), [1]);
    assert_eq!(rows.omitted(), Some(1));

    let unknown = BoundedText::from_total("é", 1, None).unwrap();
    assert_eq!(unknown.omitted_chars(), None);
    assert_eq!(
        serde_json::to_value(unknown).unwrap()["omitted_chars"],
        Value::Null
    );

    assert!(
        serde_json::from_str::<BoundedRows<u8>>(r#"{"items":[1,2],"applied":1,"omitted":0}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<BoundedRows<u8>>(r#"{"items":[1],"applied":2,"omitted":1}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<BoundedText>(
            r#"{"text":"too long","applied_chars":1,"omitted_chars":0}"#
        )
        .is_err()
    );

    let too_many = vec![0_u8; 1_001];
    assert!(
        BoundedRows::for_limit(too_many, Some(1_001), LimitId::LoopScheduledOccurrences).is_err()
    );
    assert!(matches!(
        BoundedRows::for_limit(vec![1_u8], Some(1), LimitId::PlanBodyChars),
        Err(LimitError::WrongShape { .. })
    ));
    assert!(matches!(
        root_limit(LimitId::PlanBodyChars, Some(0)),
        Err(LimitError::WrongShape { .. })
    ));
    assert!(validate_input_bytes(80_004, LimitId::PlanBodyInputBytes).is_ok());
    assert!(matches!(
        validate_input_bytes(80_005, LimitId::PlanBodyInputBytes),
        Err(LimitError::Bound(error)) if error.unit == BoundUnit::Bytes
    ));
}

#[test]
fn partial_section_error_does_not_erase_other_recorder_data() {
    let snapshot = scenarios::partial_recorder_snapshot();
    assert!(snapshot.loops.is_none());
    assert_eq!(snapshot.open_plans.len(), 1);
    assert_eq!(snapshot.timeline.len(), 1);
    assert_eq!(snapshot.errors.len(), 1);
    assert_eq!(snapshot.errors[0].scope(), "loops");
}

#[test]
fn error_scope_and_code_registries_are_exact_and_unique() {
    assert_eq!(
        SNAPSHOT_ERROR_SCOPES,
        [
            "repository",
            "state.sessions",
            "state.plans",
            "state.decisions",
            "state.receipts",
            "loops",
            "gates",
            "body",
        ]
    );
    assert_eq!(SNAPSHOT_ERROR_SCOPES.len(), 8);
    assert_eq!(SNAPSHOT_ERROR_CODES.len(), 15);
    assert_eq!(
        SNAPSHOT_ERROR_CODES
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        SNAPSHOT_ERROR_CODES.len()
    );
    for domain in [
        CollectionDomain::Repository,
        CollectionDomain::Sessions,
        CollectionDomain::Plans,
        CollectionDomain::Decisions,
        CollectionDomain::Receipts,
        CollectionDomain::Loops,
        CollectionDomain::Gates,
        CollectionDomain::Body,
    ] {
        assert!(SNAPSHOT_ERROR_SCOPES.contains(&domain.as_str()));
    }
}

#[test]
fn parity_registry_has_one_named_oracle_for_every_matrix_row() {
    let planned_capabilities = include_str!("fixtures/parity-capabilities.txt")
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        PARITY_REGISTRY
            .iter()
            .map(|entry| entry.capability)
            .collect::<Vec<_>>(),
        planned_capabilities
    );
    let keys = PARITY_REGISTRY
        .iter()
        .map(|entry| entry.key)
        .collect::<BTreeSet<_>>();
    let capabilities = PARITY_REGISTRY
        .iter()
        .map(|entry| entry.capability)
        .collect::<BTreeSet<_>>();
    assert_eq!(keys.len(), PARITY_REGISTRY.len());
    assert_eq!(capabilities.len(), PARITY_REGISTRY.len());
    assert!(PARITY_REGISTRY.iter().all(|entry| {
        !entry.key.is_empty()
            && !entry.capability.is_empty()
            && !entry.test_source.is_empty()
            && !entry.behavioral_test.is_empty()
    }));
    let mut fanout = BTreeMap::new();
    for entry in PARITY_REGISTRY {
        *fanout
            .entry((entry.test_source, entry.behavioral_test))
            .or_insert(0_usize) += 1;
    }
    assert!(
        fanout.values().all(|count| *count <= 5),
        "one parity oracle is responsible for too many unrelated rows: {fanout:?}"
    );

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..");
    for entry in PARITY_REGISTRY {
        let relative = Path::new(entry.test_source);
        assert!(
            is_safe_repository_relative_path(relative),
            "parity row {} has an unsafe test source: {}",
            entry.key,
            entry.test_source
        );
        assert_test_source_is_collected(manifest_dir, &root, entry.test_source);
        let Some(source_path) = resolve_test_source(manifest_dir, &root, entry.test_source) else {
            // Published jig-ui archives do not contain the jig CLI's integration
            // tests. Their exact allowlist is still checked above; a workspace
            // checkout resolves and validates them normally.
            continue;
        };
        let source = fs::read_to_string(&source_path).unwrap_or_else(|error| {
            panic!(
                "parity row {} cannot read test source {}: {error}",
                entry.key, entry.test_source
            )
        });
        assert!(
            source_declares_active_test(&source, entry.behavioral_test).unwrap_or_else(|error| {
                panic!(
                    "parity row {} cannot parse test source {}: {error}",
                    entry.key, entry.test_source
                )
            }),
            "parity row {} references missing, ignored, or cfg-gated test {} in {}",
            entry.key,
            entry.behavioral_test,
            entry.test_source
        );
    }
}

fn is_safe_repository_relative_path(path: &Path) -> bool {
    path.is_relative()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn resolve_test_source(manifest_dir: &Path, root: &Path, source: &str) -> Option<PathBuf> {
    if let Some(local) = source.strip_prefix("crates/jig-ui/") {
        return Some(manifest_dir.join(local));
    }
    let path = root.join(source);
    path.is_file().then_some(path)
}

fn source_declares_active_test(source: &str, test_name: &str) -> syn::Result<bool> {
    let file = syn::parse_file(source)?;
    Ok(items_declare_active_test(&file.items, test_name, false))
}

fn items_declare_active_test(items: &[syn::Item], test_name: &str, inherited_cfg: bool) -> bool {
    items.iter().any(|item| match item {
        syn::Item::Fn(function) => {
            !inherited_cfg
                && function.sig.ident == test_name
                && function
                    .attrs
                    .iter()
                    .any(|attribute| attribute.path().is_ident("test"))
                && !has_disabling_test_attribute(&function.attrs)
        }
        syn::Item::Mod(module) => module.content.as_ref().is_some_and(|(_, items)| {
            items_declare_active_test(
                items,
                test_name,
                inherited_cfg || has_disabling_cfg_attribute(&module.attrs),
            )
        }),
        _ => false,
    })
}

fn has_disabling_test_attribute(attributes: &[syn::Attribute]) -> bool {
    attributes
        .iter()
        .any(|attribute| attribute.path().is_ident("ignore"))
        || has_disabling_cfg_attribute(attributes)
}

fn has_disabling_cfg_attribute(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg_attr")
            || (attribute.path().is_ident("cfg") && !is_test_cfg(attribute))
    })
}

fn is_test_cfg(attribute: &syn::Attribute) -> bool {
    let syn::Meta::List(list) = &attribute.meta else {
        return false;
    };
    list.tokens.to_string() == "test"
}

fn source_declares_active_module(source: &str, module_name: &str) -> syn::Result<bool> {
    let file = syn::parse_file(source)?;
    Ok(file.items.iter().any(|item| {
        let syn::Item::Mod(module) = item else {
            return false;
        };
        module.ident == module_name && !has_disabling_cfg_attribute(&module.attrs)
    }))
}

fn assert_test_source_is_collected(manifest_dir: &Path, root: &Path, source: &str) {
    let (module_source, module_name) = match source {
        "crates/jig-ui/src/terminal/tests.rs" => ("crates/jig-ui/src/terminal.rs", "tests"),
        "crates/jig-ui/src/terminal/tests/local.rs" => {
            ("crates/jig-ui/src/terminal/tests.rs", "local")
        }
        "crates/jig-ui/src/terminal/tests/local/parity.rs" => {
            ("crates/jig-ui/src/terminal/tests/local.rs", "parity")
        }
        "crates/jig-ui/src/terminal/tests/regressions.rs" => {
            ("crates/jig-ui/src/terminal/tests.rs", "regressions")
        }
        "crates/jig-ui/src/terminal/tests/status.rs" => {
            ("crates/jig-ui/src/terminal/tests.rs", "status")
        }
        "crates/jig-ui/src/terminal/runtime/event_loop.rs" => {
            ("crates/jig-ui/src/terminal/runtime.rs", "event_loop")
        }
        "crates/jig-ui/src/terminal/runtime/scheduler/tests.rs" => {
            ("crates/jig-ui/src/terminal/runtime/scheduler.rs", "tests")
        }
        "crates/jig-ui/src/terminal/runtime/worker/tests.rs" => {
            ("crates/jig-ui/src/terminal/runtime/worker.rs", "tests")
        }
        "crates/jig-ui/tests/dashboard_contract.rs"
        | "crates/jig/tests/ui_cutover.rs"
        | "crates/jig/tests/ui_architecture.rs" => {
            return;
        }
        _ => panic!("parity registry uses an uncollected test source: {source}"),
    };
    let parent_path = resolve_test_source(manifest_dir, root, module_source)
        .unwrap_or_else(|| panic!("module source is unavailable: {module_source}"));
    let parent = fs::read_to_string(parent_path).unwrap();
    assert!(
        source_declares_active_module(&parent, module_name).unwrap_or_else(|error| {
            panic!("cannot parse module source {module_source}: {error}")
        }),
        "{source} is not collected: {module_source} lacks an unconditional mod {module_name}"
    );
}

#[test]
fn recorder_epochs_start_at_one_and_never_wrap() {
    assert!(RecorderEpochId::new(0).is_none());
    assert_eq!(RecorderEpochId::FIRST.get(), 1);
    assert_eq!(RecorderEpochId::FIRST.checked_next().unwrap().get(), 2);
    assert!(matches!(
        RecorderEpochId::new(u64::MAX).unwrap().checked_next(),
        Err(SourceError::InternalContract { .. })
    ));
    assert!(serde_json::from_str::<RecorderEpochId>("0").is_err());
    assert_eq!(
        serde_json::from_str::<RecorderEpochId>("1").unwrap().get(),
        1
    );
}

#[test]
fn timeline_limits_reject_invalid_requests_at_the_boundary() {
    assert_eq!(TimelineLimit::DEFAULT.get(), 120);
    assert!(matches!(
        TimelineLimit::new(0),
        Err(SourceError::InternalContract { .. })
    ));
    assert_eq!(TimelineLimit::new(500).unwrap().get(), 500);
    assert!(matches!(
        TimelineLimit::new(usize::MAX),
        Err(SourceError::InternalContract { .. })
    ));
    assert!(serde_json::from_str::<TimelineLimit>("0").is_err());
    assert!(serde_json::from_str::<TimelineLimit>("1001").is_err());
}

#[test]
fn recorder_and_plan_documents_reject_limit_metadata_drift() {
    let custom = jig_ui::dashboard::RecorderSnapshot::new(
        RecorderEpochId::FIRST,
        1_700_000_000_000,
        TimelineLimit::new(500).unwrap(),
    );
    let custom_wire = serde_json::to_value(custom).unwrap();
    assert_eq!(custom_wire["timeline_limit"], 500);
    assert_eq!(custom_wire["limits"]["timeline"]["applied"], 500);

    let mut recorder_wire = serde_json::to_value(scenarios::recorder_snapshot()).unwrap();
    recorder_wire["limits"]["open_plans"]["applied"] = Value::from(999);
    assert!(serde_json::from_value::<jig_ui::dashboard::RecorderSnapshot>(recorder_wire).is_err());

    let mut nested_wire = serde_json::to_value(scenarios::recorder_snapshot()).unwrap();
    nested_wire["failures"][0]["stderr_preview"]["applied_chars"] = Value::from(399);
    assert!(serde_json::from_value::<jig_ui::dashboard::RecorderSnapshot>(nested_wire).is_err());

    let mut plan_wire = serde_json::to_value(scenarios::plan_snapshot()).unwrap();
    plan_wire["limits"]["plan_receipts"]["applied"] = Value::from(49);
    assert!(serde_json::from_value::<jig_ui::dashboard::PlanSnapshot>(plan_wire).is_err());
}

#[test]
fn root_collections_cannot_serialize_past_their_ceiling() {
    let mut recorder = scenarios::recorder_snapshot();
    let plan = recorder.open_plans[0].clone();
    recorder.open_plans = vec![plan; LimitId::OpenPlans.ceiling() + 1];
    assert!(serde_json::to_value(recorder).is_err());

    let mut plan = scenarios::plan_snapshot();
    let decision = plan.decisions[0].clone();
    plan.decisions = vec![decision; LimitId::PlanDecisions.ceiling() + 1];
    assert!(serde_json::to_value(plan).is_err());
}

#[test]
fn snapshot_errors_use_registered_domains_and_codes() {
    let error = SnapshotError::new(
        CollectionDomain::Plans,
        SnapshotErrorCode::RecordDecodeFailed,
        Some("plan_example".to_string()),
        "invalid record",
    );
    assert_eq!(error.scope(), "state.plans");
    assert_eq!(error.code(), "record_decode_failed");
    assert_eq!(error.subject_id(), Some("plan_example"));
    assert_eq!(error.message(), "invalid record");
    assert!(serde_json::from_str::<SnapshotError>(
        r#"{"scope":"state.planz","code":"record_decode_failed","subject_id":null,"message":"bad"}"#
    )
    .is_err());
    assert!(serde_json::from_str::<SnapshotError>(
        r#"{"scope":"state.plans","code":"record_decode_faild","subject_id":null,"message":"bad"}"#
    )
    .is_err());
}

#[test]
fn source_contracts_keep_modes_bases_partitions_and_partial_data_distinct() {
    use jig_ui::dashboard::{
        Observation, PlanBasis, PlanSnapshotResult, RecorderMode, RecorderRequest, StatusPhase,
        StatusProviderSnapshot, StatusRequest,
    };

    let request = RecorderRequest {
        mode: RecorderMode::ReuseCurrent,
        timeline_limit: TimelineLimit::new(1_000).unwrap(),
    };
    assert_eq!(request.mode, RecorderMode::ReuseCurrent);
    assert_eq!(request.timeline_limit.get(), 1_000);
    let status_request = StatusRequest {
        timeline_limit: TimelineLimit::new(1).unwrap(),
    };
    assert_eq!(status_request.timeline_limit.get(), 1);
    assert_eq!(
        PlanBasis::RecorderEpoch(RecorderEpochId::FIRST),
        PlanBasis::RecorderEpoch(RecorderEpochId::FIRST)
    );
    assert_ne!(
        PlanBasis::Fresh,
        PlanBasis::RecorderEpoch(RecorderEpochId::FIRST)
    );

    let error = SnapshotError::new(
        CollectionDomain::Receipts,
        SnapshotErrorCode::StreamReadFailed,
        None,
        "receipt stream unavailable",
    );
    let partial = Observation::partial(7_u8, error.clone());
    assert_eq!(partial.data, Some(7));
    assert_eq!(partial.error, Some(error));
    let unavailable = Observation::<u8>::unavailable(SnapshotError::new(
        CollectionDomain::Body,
        SnapshotErrorCode::BodyNotFound,
        Some("plan_example".to_string()),
        "plan body missing",
    ));
    assert!(unavailable.data.is_none());
    assert_eq!(unavailable.error.unwrap().scope(), "body");

    let mut status = scenarios::status_snapshot();
    let provider_partition = StatusProviderSnapshot {
        observed_at_ms: status.observed_at_ms + 25,
        providers: std::mem::take(&mut status.providers),
        errors: std::mem::take(&mut status.errors),
    };
    assert_eq!(provider_partition.observed_at_ms, 1_700_000_000_025);
    assert_eq!(provider_partition.providers.len(), 1);
    assert!(provider_partition.errors.is_empty());

    assert_ne!(StatusPhase::Providers, StatusPhase::LocalEpoch);
    assert!(matches!(
        PlanSnapshotResult::NotFound,
        PlanSnapshotResult::NotFound
    ));
    assert!(matches!(
        PlanSnapshotResult::StaleRecorderEpoch,
        PlanSnapshotResult::StaleRecorderEpoch
    ));
    assert_eq!(
        SourceError::Cancelled.to_string(),
        "dashboard collection cancelled"
    );
}

fn assert_root_fields(document: &Value, expected: &[&str]) {
    let actual = document
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected.iter().copied().collect());
}
