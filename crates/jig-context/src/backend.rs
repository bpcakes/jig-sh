use serde::{Deserialize, Serialize};

pub const GO_POSTGRES_MIGRATION_DIR: &str = "internal/database/migrations";
pub const GO_TOOLCHAIN_AUTHORITY_PATH: &str = "go.mod";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendLanguage {
    #[default]
    Rust,
    Go,
}

impl BackendLanguage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }

    pub const fn is_go(self) -> bool {
        matches!(self, Self::Go)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GoDatabase {
    #[default]
    None,
    Postgres,
}

impl GoDatabase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Postgres => "postgres",
        }
    }

    pub const fn is_postgres(self) -> bool {
        matches!(self, Self::Postgres)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct SelectorFixture<T> {
        value: T,
    }

    #[test]
    fn backend_selectors_reject_unknown_values() {
        let language_error = toml::from_str::<SelectorFixture<BackendLanguage>>("value = \"ruby\"")
            .unwrap_err()
            .to_string();
        assert!(language_error.contains("unknown variant `ruby`"));

        let database_error = toml::from_str::<SelectorFixture<GoDatabase>>("value = \"sqlite\"")
            .unwrap_err()
            .to_string();
        assert!(database_error.contains("unknown variant `sqlite`"));
    }

    #[test]
    fn backend_selectors_keep_legacy_defaults() {
        assert_eq!(BackendLanguage::default(), BackendLanguage::Rust);
        assert_eq!(GoDatabase::default(), GoDatabase::None);

        let language: SelectorFixture<BackendLanguage> = toml::from_str("value = \"go\"").unwrap();
        let database: SelectorFixture<GoDatabase> = toml::from_str("value = \"postgres\"").unwrap();
        assert_eq!(language.value, BackendLanguage::Go);
        assert_eq!(database.value, GoDatabase::Postgres);
    }
}
