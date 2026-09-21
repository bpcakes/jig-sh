use std::collections::BTreeMap;

use jig_contract::{ActionIntent, ExecutionResourceV1};
use tempfile::TempDir;

use super::*;
use crate::repository::execution_resources;

fn fixture(web: &str, api: &str, url: &str) -> (TempDir, RepoContext, PlannedTarget) {
    let temp = tempfile::tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .repo_name("ExampleBrowserProject")
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut planned = PlannedTarget::new(
        "web:e2e".parse().unwrap(),
        ActionIntent::Check,
        ActionRunner::Shell {
            command: "example_browser_command".into(),
            working_directory: None,
            environment: BTreeMap::from([
                ("E2E_WEB_PORT".into(), web.into()),
                ("E2E_API_PORT".into(), api.into()),
                ("E2E_BASE_URL".into(), url.into()),
            ]),
        },
        "example-input",
    );
    planned.resources = vec![ExecutionResourceV1::PlaywrightServersV1 {}];
    (temp, ctx, planned)
}

fn resolved(ctx: &RepoContext, planned: &PlannedTarget) -> ResolvedResources {
    execution_resources::resolve(ctx, planned, Duration::from_secs(10), &|| false).unwrap()
}

#[test]
fn generated_playwright_port_and_external_url_semantics_match_resolution() {
    // Read the owning template's actual functions, not another hand-copied
    // Rust interpretation of JS numeric/whitespace rules.
    let template = include_str!(
        "../../../../../templates/scaffolds/rust-react/frontend/vite-react/playwright.config.ts.jinja"
    );
    let functions = &template[template.find("function readPort(").unwrap()..];
    // Node's type stripper is not a prerequisite; these fixed annotations are
    // removed only from the generated template oracle, never from application code.
    let functions = functions
        .replace(": string | undefined", "")
        .replace(": string", "")
        .replace(": number", "");
    let oracle = format!(
        "{functions}\nconst web = readPort('E2E_WEB_PORT',4173); const api = readPort('E2E_API_PORT',4174); if(web===api) process.exit(2); process.stdout.write(JSON.stringify(readOptionalEnvironmentVariable('E2E_BASE_URL') ? 0 : 2));"
    );
    for (web, api, url) in [
        ("", "", ""),
        (" ", "\t", " \n"),
        ("0x1051", "0b1000001001110", ""),
        ("4.173e3", "4174.0", ""),
        ("\u{feff}4173\u{a0}", "0o10116", ""),
        ("1", "65535", ""),
        ("0", "4174", ""),
        ("-1", "4174", ""),
        ("4173.5", "4174", ""),
        ("NaN", "4174", ""),
        ("Infinity", "4174", ""),
        ("65536", "4174", ""),
        ("4173", "4173", ""),
        ("", "", " \u{feff}https://example.invalid/check \n"),
        ("bad", "4174", "https://example.invalid/check"),
        ("4173", "4173", "https://example.invalid/check"),
    ] {
        let (_temp, ctx, planned) = fixture(web, api, url);
        let oracle_result = Command::new("node")
            .args(["--eval", &oracle])
            .env_remove("NODE_OPTIONS")
            .env_remove("NODE_PATH")
            .env("E2E_WEB_PORT", web)
            .env("E2E_API_PORT", api)
            .env("E2E_BASE_URL", url)
            .output()
            .unwrap();
        let result =
            execution_resources::resolve(&ctx, &planned, Duration::from_secs(10), &|| false);
        assert_eq!(
            result.is_ok(),
            oracle_result.status.success(),
            "web={web:?}, api={api:?}"
        );
        if let Ok(result) = result {
            let expected: usize = serde_json::from_slice(&oracle_result.stdout).unwrap();
            assert_eq!(result.claims.len(), expected);
            assert_eq!(
                result.partial_reason, None,
                "browser-only must not add Cargo fallback"
            );
            assert!(
                result
                    .claims
                    .iter()
                    .all(|claim| claim.mode == ResourceClaimMode::Exclusive)
            );
        }
    }
}

#[test]
fn endpoint_identity_is_role_and_repository_independent_but_ports_are_distinct() {
    let (_first, first_ctx, first) = fixture("43171", "43172", "");
    let (_swapped, swapped_ctx, swapped) = fixture("43172", "43171", "");
    let (_partial, partial_ctx, partial) = fixture("43172", "43173", "");
    let (_other, other_ctx, other) = fixture("43174", "43175", "");
    let first = resolved(&first_ctx, &first);
    let swapped = resolved(&swapped_ctx, &swapped);
    let partial = resolved(&partial_ctx, &partial);
    let other = resolved(&other_ctx, &other);
    assert_eq!(first.claims, swapped.claims);
    assert_eq!(
        first
            .claims
            .iter()
            .filter(|claim| partial.claims.contains(claim))
            .count(),
        1
    );
    assert!(
        first
            .claims
            .iter()
            .all(|claim| !other.claims.contains(claim))
    );
    assert!(!first.same_identity(&other));
}

#[test]
fn external_url_owns_nothing_and_does_not_enter_resource_identity() {
    let (_temp, ctx, planned) = fixture("", "", "https://example.invalid/private-path");
    let resolved = resolved(&ctx, &planned);
    assert!(resolved.claims.is_empty());
    assert_eq!(resolved.partial_reason, None);
    assert_eq!(resolved.identity, ["playwright_servers_v1"]);
    assert!(!format!("{resolved:?}").contains("private-path"));
}

#[test]
fn unavailable_node_and_exhausted_budget_never_become_partial_admission() {
    let (temp, ctx, mut planned) = fixture("", "", "https://example.invalid/private-path");
    let ActionRunner::Shell { environment, .. } = &mut planned.runner else {
        unreachable!()
    };
    environment.insert("PATH".into(), temp.path().to_str().unwrap().into());
    let error = execution_resources::resolve(&ctx, &planned, Duration::from_secs(10), &|| false)
        .unwrap_err();
    assert!(!error.to_string().contains("private-path"));
    for (timeout, cancelled, expected) in [
        (Duration::ZERO, false, CargoResourceStop::TimedOut),
        (Duration::from_secs(10), true, CargoResourceStop::Cancelled),
    ] {
        let error =
            execution_resources::resolve(&ctx, &planned, timeout, &|| cancelled).unwrap_err();
        assert_eq!(error.downcast_ref::<CargoResourceStop>(), Some(&expected));
    }
}
