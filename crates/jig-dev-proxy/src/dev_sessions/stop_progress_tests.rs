use super::attach_replacement_stop_warnings;

#[test]
fn failed_replacement_stop_keeps_accumulated_warnings_in_the_error_chain() {
    let warnings = vec![
        "session 'dev_example': authenticated stop was unavailable".to_owned(),
        "session 'dev_other': cleanup identity remained uncertain".to_owned(),
    ];

    let error =
        attach_replacement_stop_warnings(anyhow::anyhow!("later state read failed"), &warnings);
    let chain = format!("{error:#}");

    assert!(chain.contains(&warnings[0]), "{chain}");
    assert!(chain.contains(&warnings[1]), "{chain}");
    assert!(chain.contains("later state read failed"), "{chain}");
}
