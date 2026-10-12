use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, bail};

pub fn validate_staged_runtime_contract(
    destination: &Path,
    manifest_contract_version: u32,
) -> Result<()> {
    let requires_repository_scoped_runtime =
        manifest_contract_version > jig_context::LAST_VERSION_LOCKED_CONTRACT_VERSION;
    let launcher_path = destination.join("scripts/jig");
    let launcher = match fs::read_to_string(&launcher_path) {
        Ok(launcher) => Some(launcher),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to read {}", launcher_path.display()));
        }
    };

    if let Some(launcher) = launcher {
        let inspection = crate::runtime_artifacts::inspect_launcher(&launcher);
        match inspection.declared_contract_version() {
            crate::runtime_artifacts::ParsedField::Value(launcher_contract_version)
                if launcher_contract_version != manifest_contract_version =>
            {
                bail!(
                    "Staged launcher {} declares contract {}, but the staged manifest declares contract {}",
                    launcher_path.display(),
                    launcher_contract_version,
                    manifest_contract_version
                );
            }
            crate::runtime_artifacts::ParsedField::Malformed => {
                bail!(
                    "Staged launcher {} has an unreadable CONTRACT_VERSION",
                    launcher_path.display()
                );
            }
            crate::runtime_artifacts::ParsedField::Missing
                if requires_repository_scoped_runtime =>
            {
                bail!(
                    "Staged contract-v{} launcher {} does not declare CONTRACT_VERSION",
                    manifest_contract_version,
                    launcher_path.display()
                );
            }
            crate::runtime_artifacts::ParsedField::Missing
            | crate::runtime_artifacts::ParsedField::Value(_) => {}
        }

        if requires_repository_scoped_runtime && !inspection.uses_repository_scope_protocol() {
            bail!(
                "Staged contract-v{} launcher {} does not implement the repository-scoped runtime protocol",
                manifest_contract_version,
                launcher_path.display()
            );
        }
    }

    if requires_repository_scoped_runtime {
        let installer_path = destination.join("scripts/install-jig.sh");
        let installer = match fs::read_to_string(&installer_path) {
            Ok(installer) => Some(installer),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to read {}", installer_path.display()));
            }
        };
        if let Some(installer) = installer
            && !crate::runtime_artifacts::inspect_installer(&installer)
                .uses_repository_scope_protocol()
        {
            bail!(
                "Staged contract-v{} installer {} does not implement the repository-scoped runtime protocol",
                manifest_contract_version,
                installer_path.display()
            );
        }
    }
    Ok(())
}
