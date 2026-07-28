use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde_json::{Value, json};

const DEFAULT_TEST_CONFIG: &str = r#"_src_path = "/tmp/template"
_commit = "abc123"
default_branch = "main"
"#;

const DEFAULT_TEST_REPO_NAME: &str = "demo";
const DEFAULT_TEST_JIG_VERSION: &str = "0.2.0-beta.1";

/// Writes the common minimum Jig repository fixture used by non-parser tests.
///
/// Tests that exercise configuration parsing should continue to write their TOML
/// directly so the input under test remains visible at the call site.
pub(crate) struct TestRepoBuilder<'a> {
    root: &'a Path,
    config: String,
    repo_name: String,
    jig_version: String,
    contract_version: u32,
    required_commands: Vec<String>,
    tools: Vec<Value>,
}

impl<'a> TestRepoBuilder<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        Self {
            root,
            config: String::new(),
            repo_name: DEFAULT_TEST_REPO_NAME.into(),
            jig_version: DEFAULT_TEST_JIG_VERSION.into(),
            contract_version: 3,
            required_commands: vec!["contract_check_command".into()],
            tools: Vec::new(),
        }
    }

    pub(crate) fn config(mut self, config: impl AsRef<str>) -> Self {
        self.config.push_str(config.as_ref().trim());
        self.config.push('\n');
        self
    }

    pub(crate) fn repo_name(mut self, name: impl Into<String>) -> Self {
        self.repo_name = name.into();
        self
    }

    pub(crate) fn jig_version(mut self, version: impl Into<String>) -> Self {
        self.jig_version = version.into();
        self
    }

    pub(crate) fn contract_version(mut self, version: u32) -> Self {
        self.contract_version = version;
        self
    }

    pub(crate) fn required_commands<I, S>(mut self, commands: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.required_commands = commands.into_iter().map(Into::into).collect();
        self
    }

    pub(crate) fn tool(mut self, tool: Value) -> Self {
        self.tools.push(tool);
        self
    }

    pub(crate) fn tools<I>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = Value>,
    {
        self.tools.extend(tools);
        self
    }

    fn config_contents(&self) -> String {
        let mut config = format!(
            "{DEFAULT_TEST_CONFIG}repo_name = {:?}\njig_version = {:?}\n",
            self.repo_name, self.jig_version
        );
        if !self.config.is_empty() {
            config.push('\n');
            config.push_str(&self.config);
        }
        config
    }

    /// Writes only the common config fixture when a test owns its manifest.
    pub(crate) fn write_config(self) {
        fs::write(self.root.join(".jig.toml"), self.config_contents()).unwrap();
    }

    pub(crate) fn write(self) {
        fs::create_dir_all(self.root.join(".agent")).unwrap();
        fs::write(self.root.join(".jig.toml"), self.config_contents()).unwrap();

        self.write_contract();
    }

    /// Writes only the canonical contract fixture when a test owns its TOML.
    pub(crate) fn write_contract(self) {
        fs::create_dir_all(self.root.join(".agent")).unwrap();

        let contract = json!({
            "contract_version": self.contract_version,
            "tool_namespace": "jig",
            "jig_version": self.jig_version,
            "required_commands": self.required_commands,
            "tools": self.tools,
        });
        fs::write(
            self.root.join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&contract).unwrap(),
        )
        .unwrap();
    }
}

pub(crate) fn lock_env() -> EnvLockGuard {
    // Tests mutate process-global environment; every env-mutating test must
    // hold this single crate-wide lock. Several env-driven flows also depend
    // on current_dir(), so the same guard serializes cwd mutation.
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static CWD_LOCK: Mutex<()> = Mutex::new(());
    let lock = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cwd_lock = CWD_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    EnvLockGuard {
        _jig_repo_root: EnvVarGuard::remove("JIG_REPO_ROOT"),
        _jig_invoke_cwd: EnvVarGuard::remove("JIG_INVOKE_CWD"),
        _cwd_lock: cwd_lock,
        _lock: lock,
    }
}

pub(crate) struct EnvLockGuard {
    _jig_repo_root: EnvVarGuard,
    _jig_invoke_cwd: EnvVarGuard,
    _cwd_lock: MutexGuard<'static, ()>,
    _lock: MutexGuard<'static, ()>,
}

pub(crate) struct CurrentDirGuard {
    original: PathBuf,
}

impl CurrentDirGuard {
    pub(crate) fn set(path: &Path) -> Self {
        let original = env::current_dir().unwrap();
        env::set_current_dir(path).unwrap();
        Self { original }
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        env::set_current_dir(&self.original).unwrap();
    }
}

pub(crate) struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    pub(crate) fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }

    pub(crate) fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}
