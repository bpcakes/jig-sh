//! Machine-local build claims. File existence is not ownership: never unlink a
//! claim or explicitly unlock it, because an inherited child can still own it.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResourceClaimMode {
    Shared,
    Exclusive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResourceClaim {
    pub(crate) opaque_key: String,
    pub(crate) mode: ResourceClaimMode,
}

pub(crate) struct ResourceLease {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    files: Vec<std::sync::Arc<std::fs::File>>,
}

impl ResourceLease {
    /// One nonblocking attempt; the caller owns cancellation and its deadline.
    pub(crate) fn try_acquire(claims: &[ResourceClaim]) -> Result<Option<Self>> {
        let claims = normalized_claims(claims)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            unix::try_acquire(&claims)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = claims;
            bail!("resource coordination is unsupported on this platform")
        }
    }

    /// Install before a literal-exec hook. Only this command's child clears
    /// CLOEXEC; unrelated execs cannot inherit these descriptors. The command
    /// retains ownership too, so dropping it is part of releasing the claim.
    pub(crate) fn inherit_into(&self, command: &mut Command) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            unix::inherit_into(&self.files, command)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = command;
            bail!("resource coordination is unsupported on this platform")
        }
    }
}

fn normalized_claims(claims: &[ResourceClaim]) -> Result<BTreeMap<&str, ResourceClaimMode>> {
    if claims.len() > 64 {
        bail!("resource claim count exceeds its bound");
    }
    let mut normalized = BTreeMap::new();
    for claim in claims {
        if claim.opaque_key.len() != 64
            || !claim
                .opaque_key
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("resource claim identity is not a canonical SHA-256 key");
        }
        let mode = normalized
            .entry(claim.opaque_key.as_str())
            .or_insert(claim.mode);
        if claim.mode == ResourceClaimMode::Exclusive {
            *mode = ResourceClaimMode::Exclusive;
        }
    }
    Ok(normalized)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;
