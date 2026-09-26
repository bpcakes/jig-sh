use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::super::staged_render::StagedRender;

pub(super) fn validate_pinned_runtime_scripts(
    staged: &StagedRender,
    destination: &Path,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        return Ok(());
    }
    let pin = destination.join(".jig/runtime-version");
    match fs::symlink_metadata(&pin) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to inspect {}", pin.display()));
        }
    }

    for relative in ["scripts/jig", "scripts/install-jig.sh"] {
        let relative_path = Path::new(relative);
        if staged.retirement_paths.contains(relative_path) {
            bail!(
                "Refusing to remove {relative} while .jig/runtime-version exists: the runtime pin would lose its managed scripts. No managed files were changed. Select a full pin-aware template, or remove the pin intentionally."
            );
        }
        if !staged.active_paths.contains(relative_path) {
            continue;
        }
        let rendered = fs::read_to_string(staged.destination.join(relative_path))
            .with_context(|| format!("Failed to inspect staged {relative}"))?;
        let pin_aware = if relative == "scripts/jig" {
            crate::runtime_artifacts::inspect_launcher(&rendered).supports_release_runtime_pin()
        } else {
            crate::runtime_artifacts::inspect_installer(&rendered).supports_release_runtime_pin()
        };
        if !pin_aware {
            bail!(
                "Refusing to replace {relative} while .jig/runtime-version exists: the staged script does not support the runtime pin. No managed files were changed. Select a pin-aware template source and retry, or remove the pin intentionally."
            );
        }
    }
    Ok(())
}
