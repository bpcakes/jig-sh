impl FeatureContext for RepoContext {
    fn contract_version(&self) -> u32 {
        self.contract_version()
    }

    fn required_commands(&self) -> &[String] {
        self.required_commands()
    }

    fn sqlx_enabled(&self) -> bool {
        self.sqlx_enabled()
    }

    fn schema_dump_enabled(&self) -> bool {
        self.schema_dump_enabled()
    }

    fn migration_add_enabled(&self) -> bool {
        self.migration_add_enabled()
    }

    fn sqlx_owns_migration_authoring(&self) -> bool {
        self.sqlx_owns_migration_authoring()
    }

    fn frontend_app_count(&self) -> usize {
        if self.is_minimal_footprint() {
            0
        } else {
            self.frontend_apps().len()
        }
    }

    fn go_backend_enabled(&self) -> bool {
        self.is_go_backend()
    }

    fn go_postgres_enabled(&self) -> bool {
        if self.contract_version() >= 6 {
            self.has_component_adapter("go-postgres")
        } else {
            self.is_go_backend() && self.config.go_database.is_postgres()
        }
    }

    fn migration_authoring_enabled(&self) -> bool {
        if self.contract_version() >= 6 {
            self.migration_backend()
                .is_ok_and(|backend| backend.is_some())
        } else {
            self.migration_add_enabled() || <Self as FeatureContext>::go_postgres_enabled(self)
        }
    }

    fn migration_authoring_error(&self) -> Option<String> {
        if self.contract_version() < 6 {
            return None;
        }
        match self.migration_backend() {
            Err(error) => Some(format!(
                "jig.migration_add is unavailable because repository migration ownership is invalid: {error}"
            )),
            Ok(None)
                if self.migration_policy_enabled()
                    && !(self.sqlx_enabled()
                        && !self.migration_add_enabled()
                        && !self.has_component_adapter("go-postgres")) =>
            {
                Some(
                    "jig.migration_add is unavailable because no component owns native migration authoring; add one native migration-add action to the owning SQLx or Go/PostgreSQL component, then run `jig update --recopy`."
                        .into(),
                )
            }
            Ok(None | Some(_)) => None,
        }
    }
}
