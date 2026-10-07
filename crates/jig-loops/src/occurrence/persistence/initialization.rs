use super::*;

#[derive(Clone, Copy, Eq, PartialEq)]
enum PublicScheduleAuthorityWitness {
    Public,
    ProtectedCutoverPending,
    Protected,
}

impl SchedulePersistence {
    pub(super) fn ensure_initialization_markers(
        &self,
        directories: &ScheduleDirectories,
    ) -> Result<()> {
        let protected = match self.protected_authority()? {
            Some(authority)
                if location_exists(
                    directories.protected.as_ref(),
                    &authority.path,
                    SCHEDULE_FILE_NAME,
                )? =>
            {
                Some(authority)
            }
            _ => None,
        };
        if let Some(authority) = protected {
            self.ensure_initialization_marker_at(
                directories.protected.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Protected loop schedule directory is unavailable")
                })?,
                &authority.initialized_path,
                "protected loop schedule initialization marker",
                PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
                PROTECTED_SCHEDULE_STATE_PATH,
                false,
            )?;
        }
        let (public_marker_schema, public_marker_state_path) = if protected.is_some() {
            (
                PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
                PROTECTED_SCHEDULE_STATE_PATH,
            )
        } else {
            (SCHEDULE_INITIALIZATION_SCHEMA_VERSION, SCHEDULE_STATE_PATH)
        };
        self.ensure_initialization_marker_at(
            directories.public()?,
            &self.initialized_path,
            "loop schedule initialization marker",
            public_marker_schema,
            public_marker_state_path,
            protected.is_some(),
        )
    }

    fn ensure_initialization_marker_at(
        &self,
        directory: &StateDirectory,
        path: &Path,
        description: &str,
        schema_version: u32,
        state_path: &str,
        replace_existing: bool,
    ) -> Result<()> {
        let expected = ScheduleInitializationMarker {
            schema_version,
            state_path: state_path.to_string(),
        };
        let current = directory.read_json::<ScheduleInitializationMarker>(
            OsStr::new(SCHEDULE_INITIALIZED_FILE_NAME),
            path,
            &|| false,
        );
        if replace_existing {
            if current
                .as_ref()
                .ok()
                .and_then(Option::as_ref)
                .is_some_and(|marker| {
                    marker.schema_version == expected.schema_version
                        && marker.state_path == expected.state_path
                })
            {
                return Ok(());
            }
            return write_location(directory, path, SCHEDULE_INITIALIZED_FILE_NAME, &expected);
        }
        let current = current?;
        if current.as_ref().is_some_and(|marker| {
            marker.schema_version == expected.schema_version
                && marker.state_path == expected.state_path
        }) {
            return Ok(());
        }
        let upgrading_protected_witness = current.as_ref().is_some_and(|marker| {
            schema_version == PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION
                && marker.schema_version == SCHEDULE_INITIALIZATION_SCHEMA_VERSION
                && marker.state_path == SCHEDULE_STATE_PATH
        });
        if current.is_some() && !upgrading_protected_witness {
            bail!("Invalid {description} at {}", path.display());
        }
        write_location(directory, path, SCHEDULE_INITIALIZED_FILE_NAME, &expected)
    }

    fn protected_marker_requires_authority(
        &self,
        directories: &ScheduleDirectories,
    ) -> Result<bool> {
        let Some(authority) = self.protected_authority()? else {
            return Ok(false);
        };
        let Some(marker) = read_location::<ScheduleInitializationMarker>(
            directories.protected.as_ref(),
            &authority.initialized_path,
            SCHEDULE_INITIALIZED_FILE_NAME,
            &|| false,
        )?
        else {
            return Ok(false);
        };
        match (marker.schema_version, marker.state_path.as_str()) {
            (SCHEDULE_INITIALIZATION_SCHEMA_VERSION, SCHEDULE_STATE_PATH) => Ok(false),
            (PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION, PROTECTED_SCHEDULE_STATE_PATH) => {
                Ok(true)
            }
            _ => bail!(
                "Invalid protected loop schedule initialization marker at {}",
                authority.initialized_path.display()
            ),
        }
    }

    fn public_authority_witness(
        &self,
        directories: &ScheduleDirectories,
    ) -> Result<Option<PublicScheduleAuthorityWitness>> {
        let Some(marker) = read_location::<ScheduleInitializationMarker>(
            directories.public.as_ref(),
            &self.initialized_path,
            SCHEDULE_INITIALIZED_FILE_NAME,
            &|| false,
        )?
        else {
            return Ok(None);
        };
        match (marker.schema_version, marker.state_path.as_str()) {
            (SCHEDULE_INITIALIZATION_SCHEMA_VERSION, SCHEDULE_STATE_PATH) => {
                Ok(Some(PublicScheduleAuthorityWitness::Public))
            }
            (PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION, SCHEDULE_STATE_PATH) => Ok(Some(
                PublicScheduleAuthorityWitness::ProtectedCutoverPending,
            )),
            (PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION, PROTECTED_SCHEDULE_STATE_PATH) => {
                Ok(Some(PublicScheduleAuthorityWitness::Protected))
            }
            _ => bail!(
                "Invalid loop schedule initialization marker at {}",
                self.initialized_path.display()
            ),
        }
    }

    pub(super) fn durable_read_location<'a>(
        &'a self,
        directories: &'a ScheduleDirectories,
    ) -> Result<(Option<&'a StateDirectory>, &'a Path)> {
        let Some(authority) = self.protected_authority()? else {
            if matches!(
                self.public_authority_witness(directories)?,
                Some(
                    PublicScheduleAuthorityWitness::ProtectedCutoverPending
                        | PublicScheduleAuthorityWitness::Protected
                )
            ) {
                bail!(
                    "Loop schedule initialization marker requires protected Git authority, but Git metadata is unavailable at {}",
                    self.initialized_path.display()
                );
            }
            return Ok((directories.public.as_ref(), &self.path));
        };
        if location_exists(
            directories.protected.as_ref(),
            &authority.path,
            SCHEDULE_FILE_NAME,
        )? || self.protected_marker_requires_authority(directories)?
            || self.public_authority_witness(directories)?
                == Some(PublicScheduleAuthorityWitness::Protected)
        {
            return Ok((directories.protected.as_ref(), &authority.path));
        }
        Ok((directories.public.as_ref(), &self.path))
    }

    pub(super) fn write_durable_schedule(
        &self,
        directories: &ScheduleDirectories,
        store: &ScheduleFile,
    ) -> Result<()> {
        let Some(authority) = self.protected_authority()? else {
            return write_location(directories.public()?, &self.path, SCHEDULE_FILE_NAME, store);
        };
        let protected = directories
            .protected
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Protected loop schedule directory is unavailable"))?;
        let public_witness = self.public_authority_witness(directories)?;
        if !matches!(
            public_witness,
            Some(
                PublicScheduleAuthorityWitness::ProtectedCutoverPending
                    | PublicScheduleAuthorityWitness::Protected
            )
        ) {
            write_location(directories.public()?, &self.path, SCHEDULE_FILE_NAME, store)?;
            self.ensure_initialization_marker_at(
                directories.public()?,
                &self.initialized_path,
                "loop schedule protected-authority cutover marker",
                PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
                SCHEDULE_STATE_PATH,
                true,
            )?;
        }
        write_location(protected, &authority.path, SCHEDULE_FILE_NAME, store)?;
        self.ensure_initialization_marker_at(
            protected,
            &authority.initialized_path,
            "protected loop schedule initialization marker",
            PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
            PROTECTED_SCHEDULE_STATE_PATH,
            false,
        )?;
        // Protected Git metadata is the commit point. The checkout copy is a
        // compatibility/diagnostic replica and is repaired on later writes.
        let _ = write_location(directories.public()?, &self.path, SCHEDULE_FILE_NAME, store);
        self.ensure_initialization_marker_at(
            directories.public()?,
            &self.initialized_path,
            "loop schedule initialization marker",
            PROTECTED_SCHEDULE_AUTHORITY_SCHEMA_VERSION,
            PROTECTED_SCHEDULE_STATE_PATH,
            true,
        )
    }
}
