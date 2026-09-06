fn normalize_external_check_args(
    raw: Vec<String>,
    tool: &mut ToolOpts,
    profile: &mut Option<String>,
    affected: &mut Option<String>,
    explain: &mut bool,
    fail_fast: &mut bool,
    comparison: &mut CheckComparisonOpts,
) -> Result<Vec<String>> {
    let mut selectors = Vec::new();
    let mut args = raw.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-receipt" => tool.no_receipt = true,
            "--explain" => *explain = true,
            "--fail-fast" => *fail_fast = true,
            "--comparison-staged" => comparison.comparison_staged = true,
            "--comparison-strict-inventory" => comparison.comparison_strict_inventory = true,
            "--plan-id" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--plan-id requires a value"))?;
                set_external_value(&mut tool.plan_id, value, "--plan-id")?;
            }
            "--profile" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--profile requires a value"))?;
                set_external_value(profile, value, "--profile")?;
            }
            "--affected" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--affected requires a value"))?;
                set_external_value(affected, value, "--affected")?;
            }
            "--comparison-base" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--comparison-base requires a value"))?;
                set_external_value(&mut comparison.comparison_base, value, "--comparison-base")?;
            }
            "--comparison-exact-tree" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--comparison-exact-tree requires a value"))?;
                set_external_value(&mut comparison.comparison_exact_tree, value, "--comparison-exact-tree")?;
            }
            "--comparison-provenance" => {
                let value = args.next().ok_or_else(|| anyhow::anyhow!("--comparison-provenance requires a value"))?;
                set_external_provenance(comparison, &value)?;
            }
            _ if arg.starts_with("--plan-id=") => set_external_value(&mut tool.plan_id, arg["--plan-id=".len()..].to_owned(), "--plan-id")?,
            _ if arg.starts_with("--profile=") => set_external_value(profile, arg["--profile=".len()..].to_owned(), "--profile")?,
            _ if arg.starts_with("--affected=") => set_external_value(affected, arg["--affected=".len()..].to_owned(), "--affected")?,
            _ if arg.starts_with("--comparison-base=") => set_external_value(&mut comparison.comparison_base, arg["--comparison-base=".len()..].to_owned(), "--comparison-base")?,
            _ if arg.starts_with("--comparison-exact-tree=") => set_external_value(&mut comparison.comparison_exact_tree, arg["--comparison-exact-tree=".len()..].to_owned(), "--comparison-exact-tree")?,
            _ if arg.starts_with("--comparison-provenance=") => set_external_provenance(comparison, &arg["--comparison-provenance=".len()..])?,
            _ if arg.starts_with('-') => anyhow::bail!("unknown check option '{arg}'"),
            _ => selectors.push(arg),
        }
    }
    if tool.no_receipt && tool.plan_id.is_some() {
        anyhow::bail!("--no-receipt cannot be combined with --plan-id");
    }
    Ok(selectors)
}

fn set_external_provenance(comparison: &mut CheckComparisonOpts, value: &str) -> Result<()> {
    if comparison.comparison_provenance.is_some() {
        bail!("--comparison-provenance cannot be used more than once");
    }
    comparison.comparison_provenance = Some(
        CliExactTreeProvenance::from_str(value, false).map_err(|error| anyhow::anyhow!(error))?,
    );
    Ok(())
}

fn set_external_value(target: &mut Option<String>, value: String, option: &str) -> Result<()> {
    if value.is_empty() {
        anyhow::bail!("{option} requires a non-empty value");
    }
    if target.replace(value).is_some() {
        anyhow::bail!("{option} cannot be used more than once");
    }
    Ok(())
}
