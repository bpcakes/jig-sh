use super::*;

pub(super) struct ScheduleDirectories {
    pub(super) legacy: Option<StateDirectory>,
    pub(super) public: Option<StateDirectory>,
    pub(super) protected: Option<StateDirectory>,
}

impl ScheduleDirectories {
    pub(super) fn open(persistence: &SchedulePersistence, create: bool) -> Result<Self> {
        let open = |root: &Path, dir: &Path| {
            if create {
                StateDirectory::open(root, dir).map(Some)
            } else {
                StateDirectory::open_existing(root, dir)
            }
        };
        let legacy = open(&persistence.root, &persistence.legacy_dir)?;
        let public = open(&persistence.root, &persistence.dir)?;
        let protected = persistence
            .protected_authority()?
            .map(|authority| open(&authority.root, &authority.dir))
            .transpose()?
            .flatten();
        Ok(Self {
            legacy,
            public,
            protected,
        })
    }

    pub(super) fn authority(&self) -> Option<&StateDirectory> {
        self.protected.as_ref().or(self.public.as_ref())
    }

    pub(super) fn legacy(&self) -> Result<&StateDirectory> {
        self.legacy
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Legacy loop schedule directory is unavailable"))
    }

    pub(super) fn public(&self) -> Result<&StateDirectory> {
        self.public
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Loop schedule directory is unavailable"))
    }
}
