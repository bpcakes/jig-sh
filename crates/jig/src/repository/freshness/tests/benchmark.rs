//! Process-per-sample driver used by scripts/benchmark-target-freshness.py.
use super::*;

#[test]
#[ignore = "explicit reproducible performance fixture, not a timing assertion"]
fn measurement() {
    let mode = std::env::var("JIG_FRESHNESS_BENCH_MODE").unwrap();
    if mode == "prepare" {
        let mut fixture = Fixture::new();
        let mut broad = action(
            "web:broad",
            &["apps/**", "shared/**", "docs/**", "scripts/**"],
        );
        broad
            .depends_on
            .push("shared:verify-generated".parse().unwrap());
        fixture.actions.push(broad);
        fixture.write_authority();
        for index in 0..4_000 {
            let directory = if index < 100 { "apps/web/src" } else { "docs" };
            fs::write(
                fixture
                    .root()
                    .join(format!("{directory}/example-{index:04}.txt")),
                "x".repeat(512),
            )
            .unwrap();
        }
        fs::create_dir(fixture.root().join("node_modules")).unwrap();
        for index in 0..10_000 {
            fs::write(
                fixture
                    .root()
                    .join(format!("node_modules/example-{index:05}")),
                "ignored",
            )
            .unwrap();
        }
        git(fixture.root(), &["add", "."]);
        git(
            fixture.root(),
            &["commit", "-q", "-m", "Generic 4000-file benchmark"],
        );
        println!("BENCH {}", json!({"fixture":fixture.temp.keep()}));
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("JIG_FRESHNESS_BENCH_ROOT").unwrap());
    let ctx = RepoContext::load_from_root(root).unwrap();
    let mut actions = ctx.action_specs().to_vec();
    for action in &mut actions {
        action.inputs_policy = Some(ActionInputsPolicy::Exhaustive);
    }
    let catalog = RepositoryCatalog::from_native(
        8,
        ctx.contract_digest(),
        ctx.component_specs(),
        &actions,
        ctx.profile_specs(),
        ctx.default_check_profile(),
    )
    .unwrap();
    let invocations = actions
        .iter()
        .map(|action| {
            let mut target = PlannedTarget::new(
                action.target.clone(),
                action.intent,
                action.runner.clone(),
                "benchmark-legacy-token",
            );
            target.effects = action.effects.clone();
            target.inputs = action.inputs.clone();
            target.depends_on = action.depends_on.clone();
            target
        })
        .collect::<Vec<_>>();
    let timeout: u64 = std::env::var("JIG_FRESHNESS_BENCH_TIMEOUT_MS")
        .unwrap_or_else(|_| "30000".into())
        .parse()
        .unwrap();
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_millis(timeout)),
        &|| false,
    );
    // All benchmark targets explicitly opt in, so this fallback token is unused.
    let result = collect_target_identities(
        &ctx,
        &catalog,
        &invocations,
        "unused-whole-repository-token",
        &mut budget,
    );
    let outcomes = match result {
        Ok(result) => result
            .targets
            .into_iter()
            .map(|(target, identity)| {
                (
                    target.to_string(),
                    identity
                        .map(|identity| identity.identity_digest)
                        .map_err(|error| error.reason.code),
                )
            })
            .collect::<BTreeMap<_, _>>(),
        Err(error) => BTreeMap::from([("collection".into(), Err(error.reason.code))]),
    };
    println!(
        "BENCH {}",
        json!({"stats":budget.finish_stats(),"outcomes":outcomes})
    );
}
