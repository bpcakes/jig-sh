use anyhow::Result;

#[derive(Debug)]
struct JsonOkFalse;

#[derive(Debug)]
struct VaultChildExitStatus(i32);

#[derive(Debug)]
struct ForegroundInterrupted(i32);

impl std::fmt::Display for JsonOkFalse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Command reported ok=false")
    }
}

impl std::error::Error for JsonOkFalse {}

impl std::fmt::Display for VaultChildExitStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vault child exited with status {}", self.0)
    }
}

impl std::error::Error for VaultChildExitStatus {}

impl std::fmt::Display for ForegroundInterrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Foreground process interrupted with status {}", self.0)
    }
}

impl std::error::Error for ForegroundInterrupted {}

pub(super) fn require_json_ok(required: bool, output: &serde_json::Value) -> Result<()> {
    if required && output.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err(JsonOkFalse.into());
    }
    Ok(())
}

pub(super) fn require_foreground_status(output: &serde_json::Value) -> Result<()> {
    if output
        .get("interrupted")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        let exit_status = output
            .get("exit_status")
            .and_then(serde_json::Value::as_i64)
            .filter(|status| (1..=255).contains(status))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "interrupted foreground result is missing a valid shell exit_status"
                )
            })?;
        return Err(ForegroundInterrupted(exit_status as i32).into());
    }
    require_json_ok(true, output)
}

pub(super) fn require_vault_child_status_ok(output: &serde_json::Value) -> Result<()> {
    let status = output
        .get("result")
        .and_then(|value| value.get("exit_status"))
        .and_then(serde_json::Value::as_i64);
    if status.is_none() && output.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        anyhow::bail!("vault run returned ok=false without result.exit_status");
    }
    let Some(status) = status else {
        return Ok(());
    };
    if status != 0 {
        // The CLI process exit API is limited to shell-style status bytes.
        // Preserve non-zero vault child failures while keeping output portable.
        return Err(VaultChildExitStatus(status.clamp(1, 255) as i32).into());
    }
    Ok(())
}

pub(crate) fn is_structured_json_failure(error: &anyhow::Error) -> bool {
    error.is::<JsonOkFalse>()
        || error.is::<VaultChildExitStatus>()
        || error.is::<ForegroundInterrupted>()
}

pub(crate) fn structured_error_exit_code(error: &anyhow::Error) -> Option<i32> {
    error
        .downcast_ref::<VaultChildExitStatus>()
        .map(|error| error.0)
        .or_else(|| {
            error
                .downcast_ref::<ForegroundInterrupted>()
                .map(|error| error.0)
        })
}
