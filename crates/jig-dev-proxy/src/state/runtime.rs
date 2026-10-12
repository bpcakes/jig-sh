use super::*;

impl StateStore {
    /// Missing or corrupt fields are not evidence that the serving process is
    /// absent. Preserve all runtime artifacts until explicit cleanup.
    pub(crate) fn runtime_files_present_interruptible(
        &self,
        cancelled: &impl Fn() -> bool,
    ) -> Result<LockOutcome<bool>> {
        self.with_runtime_lock_interruptible(cancelled, || {
            for path in [
                self.pid_path(),
                self.proxy_exe_path(),
                self.http_port_path(),
                self.https_port_path(),
                self.health_token_path(),
            ] {
                match fs::symlink_metadata(&path) {
                    Ok(_) => return Ok(true),
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("Failed to inspect proxy runtime file {}", path.display())
                        });
                    }
                }
            }
            Ok(false)
        })
    }

    pub(super) fn remove_runtime_files_unlocked(&self) -> Result<()> {
        remove_runtime_file(self.pid_path())?;
        remove_runtime_file(self.proxy_exe_path())?;
        remove_runtime_file(self.http_port_path())?;
        remove_runtime_file(self.https_port_path())?;
        remove_runtime_file(self.health_token_path())?;
        Ok(())
    }
}
