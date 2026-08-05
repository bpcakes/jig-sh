use super::answers::RenderAnswers;

pub(super) const fn jig_launcher(minimal_footprint: bool) -> &'static str {
    if minimal_footprint {
        "jig"
    } else {
        "scripts/jig"
    }
}

pub(super) fn generated_gates(answers: &RenderAnswers) -> Vec<String> {
    // Keep this list in sync with the check tools rendered into the harness.
    // Bootstrap adopt tests cross-check the rendered tools against this preview.
    let launcher = jig_launcher(answers.is_minimal_footprint());
    let mut gates = Vec::new();
    if answers.bootstrap_command_configured() {
        gates.push(format!("{launcher} bootstrap"));
    }
    gates.extend([
        format!("{launcher} check contract"),
        format!("{launcher} check fmt"),
        format!("{launcher} check clippy"),
        format!("{launcher} check test"),
    ]);
    if answers.sqlx_enabled() {
        gates.push(format!("{launcher} check sqlx"));
    }
    if answers.schema_dump_enabled() {
        gates.push(format!("{launcher} check schema"));
        gates.push(format!("{launcher} sqlx schema dump"));
    }
    if answers.frontend_harness_enabled() {
        gates.extend([
            format!("{launcher} check typescript-lint"),
            format!("{launcher} check typescript-typecheck"),
            format!("{launcher} check typescript-build"),
            format!("{launcher} check typescript-coverage"),
        ]);
    }
    gates.push(format!("{launcher} check agent-guides"));
    gates
}
