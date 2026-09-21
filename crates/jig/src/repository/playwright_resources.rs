//! Explicit generated-Playwright environment policy, never shell discovery.
use std::{process::Command, time::Duration};

use anyhow::{Result, bail};
use jig_contract::{ActionRunner, PlannedTarget};
use sha2::{Digest, Sha256};

use super::{cargo_resources::CargoResourceStop, execution_resources::ResolvedResources};
use crate::{
    context::{CommandOutputLimit, RepoContext},
    execution::{
        ExecutionCancellation, ExecutionObserver, SupervisedExecutionError,
        run_supervised_execution_command,
    },
    repository_path::{resolve_repository_working_directory, validate_runner_environment},
    state::{ResourceClaim, ResourceClaimMode},
};

// Mirror the generated configuration's JS coercion exactly, including Unicode
// trim and hexadecimal/exponent spellings. No application module is loaded.
const ENDPOINT_PROBE: &str = r#"
function port(name, fallback) {
  const value = Number(process.env[name]?.trim() || fallback);
  if (!Number.isInteger(value) || value < 1 || value > 65535) process.exit(2);
  return value;
}
const web = port('E2E_WEB_PORT', 4173);
const api = port('E2E_API_PORT', 4174);
if (web === api) process.exit(2);
process.stdout.write(JSON.stringify(process.env.E2E_BASE_URL?.trim() ? null : [web, api]));
"#;

struct ProbeControl<'a>(&'a dyn Fn() -> bool);
impl ExecutionObserver for ProbeControl<'_> {}
impl ExecutionCancellation for ProbeControl<'_> {
    fn cancelled(&self) -> bool {
        (self.0)()
    }
}

pub(super) fn resolve(
    ctx: &RepoContext,
    planned: &PlannedTarget,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<ResolvedResources> {
    let (working_directory, environment) = match &planned.runner {
        ActionRunner::Command {
            working_directory,
            environment,
            ..
        }
        | ActionRunner::Shell {
            working_directory,
            environment,
            ..
        }
        | ActionRunner::Argv {
            working_directory,
            environment,
            ..
        } => (working_directory.as_deref(), environment),
        _ => bail!("browser endpoint policy requires a declared process runner"),
    };
    validate_runner_environment(environment)
        .map_err(|_| anyhow::anyhow!("browser endpoint environment could not be established"))?;
    let cwd =
        resolve_repository_working_directory(ctx.root(), working_directory).map_err(|_| {
            anyhow::anyhow!("browser endpoint working directory could not be established")
        })?;
    let mut command = Command::new("node");
    command
        .current_dir(cwd)
        .envs(environment)
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_PATH")
        .args(["--eval", ENDPOINT_PROBE]);
    let output = run_supervised_execution_command(
        &mut command,
        timeout,
        CommandOutputLimit::from_bytes(1024).expect("valid endpoint output bound"),
        "browser endpoint policy",
        &mut ProbeControl(cancelled),
    );
    let output = match output {
        Ok(output) if output.status.success() => output,
        Err(
            SupervisedExecutionError::CancelledBeforeStart | SupervisedExecutionError::Cancelled,
        ) => return Err(CargoResourceStop::Cancelled.into()),
        Err(SupervisedExecutionError::TimedOut) => return Err(CargoResourceStop::TimedOut.into()),
        _ => bail!(
            "browser endpoint policy could not be resolved; check Node and distinct E2E ports in 1..65535"
        ),
    };
    let ports: Option<[u16; 2]> = serde_json::from_slice(&output.stdout)
        .map_err(|_| anyhow::anyhow!("browser endpoint policy returned invalid authority"))?;
    let mut claims = Vec::new();
    if let Some([web, api]) = ports {
        if web == 0 || api == 0 || web == api {
            bail!("browser endpoint policy returned invalid ports");
        }
        for port in [web, api] {
            let mut digest = Sha256::new();
            digest.update(b"jig-loopback-endpoint-v1\0");
            digest.update(b"127.0.0.1\0");
            digest.update(port.to_be_bytes());
            claims.push(ResourceClaim {
                opaque_key: format!("{:x}", digest.finalize()),
                mode: ResourceClaimMode::Exclusive,
            });
        }
        claims.sort_by(|left, right| left.opaque_key.cmp(&right.opaque_key));
    }
    let mut identity = vec!["playwright_servers_v1".into()];
    identity.extend(claims.iter().map(|claim| claim.opaque_key.clone()));
    Ok(ResolvedResources {
        claims,
        partial_reason: None,
        identity,
    })
}

#[cfg(test)]
mod tests;
