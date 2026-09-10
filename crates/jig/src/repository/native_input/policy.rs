use super::*;

pub(super) fn prepare_policy(
    ctx: &RepoContext,
    view: CurrentViewV1,
    current_date: PolicyDateV1,
) -> PolicyPreparationV1 {
    prepare_policy_from_bytes(read_policy_bytes(ctx, view), current_date)
}

pub(super) fn prepare_policy_from_bytes(
    bytes: std::result::Result<Option<Vec<u8>>, String>,
    current_date: PolicyDateV1,
) -> PolicyPreparationV1 {
    let bytes = match bytes {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            return invalid_policy(
                None,
                PolicyPreparationFailureV1::Missing,
                vec![PreparedDiagnosticV1 {
                    severity: FindingSeverity::Error,
                    code: "file_budget.policy_invalid".into(),
                    message: format!("required file-budget policy '{POLICY_PATH_V1}' is missing"),
                    path: Some(POLICY_PATH_V1.into()),
                }],
            );
        }
        Err(message) => {
            return invalid_policy(
                None,
                PolicyPreparationFailureV1::Unreadable,
                vec![PreparedDiagnosticV1 {
                    severity: FindingSeverity::Error,
                    code: "file_budget.policy_invalid".into(),
                    message,
                    path: Some(POLICY_PATH_V1.into()),
                }],
            );
        }
    };
    match parse_policy_v1(&bytes, current_date) {
        Ok(policy) => PolicyPreparationV1::Ready {
            policy_raw_digest: format!("sha256:{}", policy.identity().raw_sha256()),
            policy_semantic_digest: format!("sha256:{}", policy.identity().semantic_sha256()),
        },
        Err(error) => invalid_policy(
            Some(format!("sha256:{}", error.raw_sha256())),
            PolicyPreparationFailureV1::Invalid,
            error
                .diagnostics()
                .iter()
                .map(prepared_diagnostic)
                .collect(),
        ),
    }
}

pub(crate) fn read_policy_bytes(
    ctx: &RepoContext,
    view: CurrentViewV1,
) -> std::result::Result<Option<Vec<u8>>, String> {
    if view == CurrentViewV1::Index {
        return match read_index_blob_v1(ctx.root(), POLICY_PATH_V1, MAX_POLICY_BYTES_V1 + 1) {
            Ok(bytes) => validate_index_policy_bytes(bytes),
            Err(error) => Err(bounded_message(&format!(
                "file-budget policy could not be read from the index: {}",
                redact_root(ctx, &format!("{error:#}"))
            ))),
        };
    }
    let path = ctx.root().join(POLICY_PATH_V1);
    let before = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("file-budget policy metadata could not be read".into()),
    };
    if !before.file_type().is_file() || before.file_type().is_symlink() {
        return Err("file-budget policy must be a regular file and may not be a symlink".into());
    }
    if before.len() > MAX_POLICY_BYTES_V1 as u64 {
        return Err(format!(
            "file-budget policy is {} bytes; preparation permits at most {MAX_POLICY_BYTES_V1}",
            before.len()
        ));
    }
    let mut file = File::open(&path).map_err(|_| "file-budget policy could not be opened")?;
    let mut bytes = Vec::with_capacity(before.len() as usize);
    file.by_ref()
        .take((MAX_POLICY_BYTES_V1 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "file-budget policy could not be read completely")?;
    let after = file
        .metadata()
        .map_err(|_| "file-budget policy identity could not be rechecked")?;
    if bytes.len() > MAX_POLICY_BYTES_V1 {
        return Err(format!(
            "file-budget policy exceeds the {MAX_POLICY_BYTES_V1}-byte preparation limit"
        ));
    }
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || !after.file_type().is_file()
    {
        return Err("file-budget policy changed while it was being prepared".into());
    }
    Ok(Some(bytes))
}

pub(super) fn validate_index_policy_bytes(
    bytes: Option<Vec<u8>>,
) -> std::result::Result<Option<Vec<u8>>, String> {
    match bytes {
        Some(bytes) if bytes.len() > MAX_POLICY_BYTES_V1 => Err(format!(
            "file-budget policy exceeds the {MAX_POLICY_BYTES_V1}-byte preparation limit (observed at least {} bytes)",
            bytes.len()
        )),
        bytes => Ok(bytes),
    }
}

fn prepared_diagnostic(diagnostic: &BudgetDiagnosticV1) -> PreparedDiagnosticV1 {
    PreparedDiagnosticV1 {
        severity: match diagnostic.severity {
            BudgetSeverityV1::Error => FindingSeverity::Error,
            BudgetSeverityV1::Warning => FindingSeverity::Warning,
            BudgetSeverityV1::Notice => FindingSeverity::Notice,
        },
        code: diagnostic.code.as_str().into(),
        message: bounded_message(&diagnostic.message),
        path: diagnostic.path.as_deref().map(bounded_message),
    }
}

fn invalid_policy(
    policy_raw_digest: Option<String>,
    reason: PolicyPreparationFailureV1,
    diagnostics: Vec<PreparedDiagnosticV1>,
) -> PolicyPreparationV1 {
    let diagnostics_count = diagnostics.len() as u64;
    let diagnostics_digest = digest_json(
        b"jig-file-budget-preparation-diagnostics-v1\0",
        &diagnostics,
    );
    let diagnostics_preview = diagnostics
        .into_iter()
        .take(MAX_PREPARED_DIAGNOSTICS_V1)
        .collect();
    PolicyPreparationV1::InvalidPolicy {
        policy_raw_digest,
        reason,
        diagnostics_count,
        diagnostics_digest,
        diagnostics_preview,
    }
}
