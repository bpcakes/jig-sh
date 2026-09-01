use std::time::Instant;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::context::RepoContext;
use crate::runtime::loops::authority::{
    ProtectedLoopAuthority, resolve_protected_loop_authority,
    resolve_protected_repository_authority,
};

use super::json_cache::{
    read_json_cache_locked_until, read_json_cache_or_default_with_cancellation,
    recover_unparsable_json_cache, replace_unparsable_json_cache,
    with_json_cache_lock_compensating_until, with_json_cache_lock_until,
};
use super::{JsonLocation, JsonWriteMode, LOOP_CACHE_DIR, loop_state_lock_deadline};

const PROTECTED_STATE_SCHEMA_VERSION: u32 = 1;
const LEGACY_MIGRATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone)]
pub(super) struct JsonStatePersistence {
    legacy: JsonLocation,
    protected: std::result::Result<Option<JsonLocation>, String>,
    protected_state_path: String,
}

impl JsonStatePersistence {
    pub(super) fn new(ctx: &RepoContext, name: &str) -> Self {
        Self::new_with_authority(ctx, name, resolve_protected_loop_authority)
    }

    pub(super) fn new_repository(ctx: &RepoContext, name: &str) -> Self {
        Self::new_with_authority(ctx, name, resolve_protected_repository_authority)
    }

    fn new_with_authority(
        ctx: &RepoContext,
        name: &str,
        resolve: fn(&std::path::Path) -> Result<Option<ProtectedLoopAuthority>>,
    ) -> Self {
        let legacy_dir = ctx.root().join(LOOP_CACHE_DIR);
        let legacy = JsonLocation::new(
            ctx.root().to_path_buf(),
            legacy_dir,
            name,
            JsonWriteMode::Cache,
        );
        let protected_state_path = format!("jig/loop/{name}.json");
        let protected = resolve(ctx.root())
            .map(|authority| authority.map(|authority| protected_location(authority, name)))
            .map_err(|error| format!("{error:#}"));
        Self {
            legacy,
            protected,
            protected_state_path,
        }
    }

    pub(super) fn with_locked<T, S>(&self, action: impl FnOnce(&mut S) -> Result<T>) -> Result<T>
    where
        S: Clone + Default + DeserializeOwned + Serialize,
    {
        self.with_locked_until(loop_state_lock_deadline(), action)
    }

    pub(super) fn with_locked_with_cancellation<T, S>(
        &self,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&mut S) -> Result<T>,
    ) -> Result<T>
    where
        S: Clone + Default + DeserializeOwned + Serialize,
    {
        self.with_locked_until_with_cancellation(loop_state_lock_deadline(), cancelled, action)
    }

    pub(super) fn with_locked_until<T, S>(
        &self,
        deadline: Instant,
        action: impl FnOnce(&mut S) -> Result<T>,
    ) -> Result<T>
    where
        S: Clone + Default + DeserializeOwned + Serialize,
    {
        self.with_locked_until_with_cancellation(deadline, &|| false, action)
    }

    fn with_locked_until_with_cancellation<T, S>(
        &self,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&mut S) -> Result<T>,
    ) -> Result<T>
    where
        S: Clone + Default + DeserializeOwned + Serialize,
    {
        let Some(protected) = self.protected()? else {
            return with_json_cache_lock_until(&self.legacy, deadline, cancelled, action);
        };
        self.ensure_initialized::<S>(protected, deadline, cancelled)?;
        with_json_cache_lock_until(
            protected,
            deadline,
            cancelled,
            |primary: &mut ProtectedState<S>| {
                primary.require_initialized()?;
                action(&mut primary.state)
            },
        )
    }

    pub(super) fn with_locked_compensating<T, U, S>(
        &self,
        cancelled: &dyn Fn() -> bool,
        action: impl FnOnce(&mut S) -> Result<T>,
        after_commit: impl FnOnce(&T, Instant) -> Result<U>,
    ) -> Result<(T, U)>
    where
        S: Clone + Default + DeserializeOwned + Serialize,
    {
        let deadline = loop_state_lock_deadline();
        let Some(protected) = self.protected()? else {
            return with_json_cache_lock_compensating_until(
                &self.legacy,
                deadline,
                cancelled,
                action,
                |result| after_commit(result, deadline),
            );
        };
        self.ensure_initialized::<S>(protected, deadline, cancelled)?;
        with_json_cache_lock_compensating_until(
            protected,
            deadline,
            cancelled,
            |primary: &mut ProtectedState<S>| {
                primary.require_initialized()?;
                action(&mut primary.state)
            },
            |result| after_commit(result, deadline),
        )
    }

    pub(super) fn read_only_with_cancellation<S>(&self, cancelled: &dyn Fn() -> bool) -> Result<S>
    where
        S: Clone + Default + DeserializeOwned,
    {
        if let Some(protected) = self.protected()? {
            let primary = self.read_protected::<S>(protected, cancelled)?;
            if primary.is_initialized()? {
                return Ok(primary.state);
            }
        }
        self.read_legacy::<S>(cancelled)?
            .state(&self.protected_state_path)
    }

    pub(super) fn read_locked_with_cancellation<S>(&self, cancelled: &dyn Fn() -> bool) -> Result<S>
    where
        S: Clone + Default + DeserializeOwned + Serialize,
    {
        let deadline = loop_state_lock_deadline();
        let Some(protected) = self.protected()? else {
            return read_json_cache_locked_until(
                &self.legacy.root,
                &self.legacy.dir,
                &self.legacy.lock_path,
                &self.legacy.path,
                deadline,
                cancelled,
            );
        };
        self.ensure_initialized::<S>(protected, deadline, cancelled)?;
        let primary: ProtectedState<S> = read_json_cache_locked_until(
            &protected.root,
            &protected.dir,
            &protected.lock_path,
            &protected.path,
            deadline,
            cancelled,
        )?;
        primary.require_initialized()?;
        Ok(primary.state)
    }

    pub(super) fn recover_unparsable_with_cancellation<S>(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<bool>
    where
        S: Clone + Default + DeserializeOwned + Serialize,
    {
        let Some(protected) = self.protected()? else {
            return recover_unparsable_json_cache::<S>(&self.legacy, cancelled);
        };
        match self.read_protected::<S>(protected, cancelled) {
            Ok(primary) if primary.is_initialized()? => Ok(false),
            Ok(_) => match self.read_legacy::<S>(cancelled) {
                Ok(_) => {
                    self.with_locked_with_cancellation::<_, S>(cancelled, |_| Ok(()))?;
                    Ok(false)
                }
                Err(error) if is_json_error(&error) => {
                    replace_unparsable_json_cache(
                        &self.legacy,
                        LegacyState::State(S::default()),
                        cancelled,
                    )?;
                    self.with_locked_with_cancellation::<_, S>(cancelled, |_| Ok(()))?;
                    Ok(true)
                }
                Err(error) => Err(error),
            },
            Err(error) if is_json_error(&error) => replace_unparsable_json_cache(
                protected,
                ProtectedState::new(S::default()),
                cancelled,
            ),
            Err(error) => Err(error),
        }
    }

    #[cfg(test)]
    pub(super) fn legacy_path(&self) -> &std::path::Path {
        &self.legacy.path
    }

    #[cfg(test)]
    pub(super) fn protected_path(&self) -> Result<Option<&std::path::Path>> {
        Ok(self.protected()?.map(|location| location.path.as_path()))
    }

    #[cfg(test)]
    pub(super) fn protected_write_mode(&self) -> Result<Option<JsonWriteMode>> {
        Ok(self.protected()?.map(|location| location.write_mode))
    }

    fn protected(&self) -> Result<Option<&JsonLocation>> {
        self.protected
            .as_ref()
            .map(Option::as_ref)
            .map_err(|error| anyhow::anyhow!(error.clone()))
    }

    fn read_protected<S>(
        &self,
        protected: &JsonLocation,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<ProtectedState<S>>
    where
        S: Default + DeserializeOwned,
    {
        read_json_cache_or_default_with_cancellation(
            &protected.root,
            &protected.dir,
            &protected.path,
            cancelled,
        )
    }

    fn read_legacy<S>(&self, cancelled: &dyn Fn() -> bool) -> Result<LegacyState<S>>
    where
        S: Default + DeserializeOwned,
    {
        read_json_cache_or_default_with_cancellation(
            &self.legacy.root,
            &self.legacy.dir,
            &self.legacy.path,
            cancelled,
        )
    }

    fn ensure_initialized<S>(
        &self,
        protected: &JsonLocation,
        deadline: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<()>
    where
        S: Clone + Default + DeserializeOwned + Serialize,
    {
        with_json_cache_lock_until(
            protected,
            deadline,
            cancelled,
            |primary: &mut ProtectedState<S>| {
                if primary.is_initialized()? {
                    return Ok(());
                }
                let state = with_json_cache_lock_until(
                    &self.legacy,
                    deadline,
                    cancelled,
                    |legacy: &mut LegacyState<S>| {
                        let state = legacy.clone().state(&self.protected_state_path)?;
                        *legacy = LegacyState::Migration(MigrationState {
                            schema_version: LEGACY_MIGRATION_SCHEMA_VERSION,
                            protected_state_path: self.protected_state_path.clone(),
                            state: state.clone(),
                        });
                        Ok(state)
                    },
                )?;
                *primary = ProtectedState::new(state);
                Ok(())
            },
        )
    }
}

fn protected_location(authority: ProtectedLoopAuthority, name: &str) -> JsonLocation {
    JsonLocation::new(authority.root, authority.dir, name, JsonWriteMode::Durable)
}

fn is_json_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<serde_json::Error>().is_some()
}

#[derive(Clone, Deserialize, Serialize)]
struct ProtectedState<S> {
    schema_version: u32,
    state: S,
}

impl<S: Default> Default for ProtectedState<S> {
    fn default() -> Self {
        Self {
            schema_version: 0,
            state: S::default(),
        }
    }
}

impl<S> ProtectedState<S> {
    fn new(state: S) -> Self {
        Self {
            schema_version: PROTECTED_STATE_SCHEMA_VERSION,
            state,
        }
    }

    fn is_initialized(&self) -> Result<bool> {
        match self.schema_version {
            0 => Ok(false),
            PROTECTED_STATE_SCHEMA_VERSION => Ok(true),
            version => bail!(
                "Unsupported protected loop state schema version {version}; expected {PROTECTED_STATE_SCHEMA_VERSION}"
            ),
        }
    }

    fn require_initialized(&self) -> Result<()> {
        if self.is_initialized()? {
            Ok(())
        } else {
            bail!("Protected loop state is not initialized")
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum LegacyState<S> {
    Migration(MigrationState<S>),
    State(S),
}

impl<S: Default> Default for LegacyState<S> {
    fn default() -> Self {
        Self::State(S::default())
    }
}

impl<S> LegacyState<S> {
    fn state(self, expected_protected_state_path: &str) -> Result<S> {
        match self {
            Self::State(state) => Ok(state),
            Self::Migration(marker) => {
                if marker.schema_version != LEGACY_MIGRATION_SCHEMA_VERSION {
                    bail!(
                        "Unsupported loop state migration schema version {}; expected {LEGACY_MIGRATION_SCHEMA_VERSION}",
                        marker.schema_version
                    );
                }
                if marker.protected_state_path != expected_protected_state_path {
                    bail!(
                        "Loop state migration marker points to {}; expected {expected_protected_state_path}",
                        marker.protected_state_path
                    );
                }
                Ok(marker.state)
            }
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct MigrationState<S> {
    schema_version: u32,
    protected_state_path: String,
    state: S,
}
