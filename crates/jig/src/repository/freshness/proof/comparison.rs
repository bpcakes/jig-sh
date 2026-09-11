use super::*;

pub(super) fn compare(
    result: &mut TargetFreshness,
    receipt: &TargetReceiptStatus,
    expected: &CollectionResult<TargetIdentityV1>,
) {
    let expected = match expected {
        Ok(expected) => expected,
        Err(error) => {
            result.status = result.status.combine(Status::Unknown);
            result.reasons.push(error.reason.clone());
            return;
        }
    };
    result.current_identity = Some(expected.into());
    let Some((recorded, _)) = complete(receipt) else {
        return;
    };
    if !bounded_identity(recorded) {
        add(result, Status::Unknown, Code::DependencyProofInvalid);
        return;
    }
    if recorded.contract_epoch != expected.contract_epoch
        || recorded.schema_version != expected.schema_version
        || recorded.digest_domain != expected.digest_domain
    {
        add(result, Status::Stale, Code::AuthorityVersionChanged);
        return;
    }
    for (different, reason) in [
        (
            recorded.configuration_digest != expected.configuration_digest,
            Code::ConfigurationChanged,
        ),
        (
            recorded.runner_digest != expected.runner_digest,
            Code::RunnerChanged,
        ),
        (
            recorded.invocation_digest != expected.invocation_digest,
            Code::InvocationChanged,
        ),
        (
            recorded.dependency_digest != expected.dependency_digest,
            Code::DependencyChanged,
        ),
    ] {
        if different {
            add(result, Status::Stale, reason);
        }
    }
    if recorded.source_digest != expected.source_digest {
        result.status = result.status.combine(Status::Stale);
        let recorded_paths: BTreeMap<_, _> = recorded
            .source_preview
            .iter()
            .map(|entry| (&entry.path, &entry.digest))
            .collect();
        let current_paths: BTreeMap<_, _> = expected
            .source_preview
            .iter()
            .map(|entry| (&entry.path, &entry.digest))
            .collect();
        let mut attributed = false;
        for path in recorded_paths
            .keys()
            .chain(current_paths.keys())
            .copied()
            .collect::<BTreeSet<_>>()
        {
            let before = recorded_paths.get(path);
            let after = current_paths.get(path);
            if before != after
                && ((before.is_some() && after.is_some())
                    || (!recorded.source_preview_truncated && !expected.source_preview_truncated))
            {
                result.reasons.push(FreshnessReason {
                    code: Code::DirectInputChanged,
                    target: None,
                    path: Some(path.clone()),
                });
                attributed = true;
            }
        }
        if !attributed {
            add(result, Status::Stale, Code::DirectInputChanged);
        }
    }
    if recorded.identity_digest != expected.identity_digest && result.status == Status::Fresh {
        // Complete comparable tokens disagree, but the retained components do
        // not justify attributing that discrepancy to a source path.
        add(result, Status::Unknown, Code::DependencyProofInvalid);
    }
}
