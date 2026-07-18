use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

#[derive(Debug, Eq, PartialEq)]
enum WindowsCommandPlan {
    Direct(OsString),
    CommandInterpreter {
        command_interpreter: OsString,
        command_line: String,
    },
}

#[derive(Debug, Eq, PartialEq)]
struct ResolvedWindowsExecutable {
    launch_path: PathBuf,
    canonical_path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsSearchPathEntryKind {
    Empty,
    DriveRelative,
    Absolute,
    Relative,
}

#[derive(Clone, Debug)]
struct WindowsEnvironment {
    path: OsString,
    path_extensions: OsString,
    command_interpreter: Option<OsString>,
}

impl WindowsEnvironment {
    fn effective(overrides: &[(String, String)]) -> Self {
        Self {
            path: effective_environment_value(overrides, "PATH").unwrap_or_default(),
            path_extensions: effective_environment_value(overrides, "PATHEXT")
                .unwrap_or_else(|| OsString::from(DEFAULT_PATHEXT)),
            command_interpreter: effective_environment_value(overrides, "COMSPEC"),
        }
    }
}

fn effective_environment_value(overrides: &[(String, String)], name: &str) -> Option<OsString> {
    overrides
        .iter()
        .rev()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| OsString::from(value))
        .or_else(|| {
            std::env::vars_os()
                .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        })
}

fn plan_windows_command(
    argv: &[String],
    working_directory: &Path,
    environment: &WindowsEnvironment,
) -> Result<WindowsCommandPlan> {
    plan_windows_command_with_interpreter(
        argv,
        working_directory,
        environment,
        resolve_command_interpreter,
    )
}

fn plan_windows_command_with_interpreter(
    argv: &[String],
    working_directory: &Path,
    environment: &WindowsEnvironment,
    resolve_interpreter: impl FnOnce(Option<&OsStr>) -> Result<OsString>,
) -> Result<WindowsCommandPlan> {
    let Some(requested) = argv.first() else {
        bail!("development app command must not be empty");
    };
    let resolved = resolve_windows_executable(
        OsStr::new(requested),
        working_directory,
        &environment.path,
        &environment.path_extensions,
    )?
    .ok_or_else(|| {
        anyhow::anyhow!(
            "Windows development command executable `{requested}` could not be resolved; ensure it names an existing regular executable or .cmd/.bat shim in the app directory or PATH"
        )
    })?;
    plan_resolved_windows_command_with_interpreter(
        resolved,
        &argv[1..],
        environment,
        resolve_interpreter,
    )
}

fn plan_resolved_windows_command(
    resolved: ResolvedWindowsExecutable,
    arguments: &[String],
    environment: &WindowsEnvironment,
) -> Result<WindowsCommandPlan> {
    plan_resolved_windows_command_with_interpreter(
        resolved,
        arguments,
        environment,
        resolve_command_interpreter,
    )
}

fn plan_resolved_windows_command_with_interpreter(
    resolved: ResolvedWindowsExecutable,
    arguments: &[String],
    environment: &WindowsEnvironment,
    resolve_interpreter: impl FnOnce(Option<&OsStr>) -> Result<OsString>,
) -> Result<WindowsCommandPlan> {
    if !is_command_shim(&resolved.launch_path) {
        return Ok(WindowsCommandPlan::Direct(
            resolved.canonical_path.into_os_string(),
        ));
    }
    let command_shim = command_shim_launch_path(&resolved.launch_path)?;

    Ok(WindowsCommandPlan::CommandInterpreter {
        command_interpreter: resolve_interpreter(environment.command_interpreter.as_deref())?,
        command_line: encode_command_shim_invocation(&command_shim, arguments)?,
    })
}

fn resolve_windows_executable(
    requested: &OsStr,
    working_directory: &Path,
    search_path: &OsStr,
    path_extensions: &OsStr,
) -> Result<Option<ResolvedWindowsExecutable>> {
    resolve_windows_executable_with(
        requested,
        working_directory,
        search_path,
        path_extensions,
        inspect_windows_executable_candidate,
    )
}

fn resolve_windows_executable_with(
    requested: &OsStr,
    working_directory: &Path,
    search_path: &OsStr,
    path_extensions: &OsStr,
    mut inspect: impl FnMut(&Path) -> std::io::Result<Option<ResolvedWindowsExecutable>>,
) -> Result<Option<ResolvedWindowsExecutable>> {
    let requested_text = requested
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Windows development command must be valid Unicode"))?;
    if requested_text.is_empty() || requested_text.chars().any(char::is_control) {
        bail!("unsafe Windows development command executable");
    }
    if is_drive_relative_windows_path(requested_text) {
        bail!(
            "Windows development command executable must not use a drive-relative path; use an app-relative or absolute path instead"
        );
    }

    let requested_path = Path::new(requested);
    let extensions = executable_extensions(requested_path, path_extensions);
    let mut candidates = Vec::new();
    let searched_path;
    if requested_path.is_absolute() {
        searched_path = false;
        append_explicit_executable_candidates(
            &mut candidates,
            requested_path.to_path_buf(),
            &extensions,
        );
    } else if requested_text.contains(['/', '\\']) {
        searched_path = false;
        append_explicit_executable_candidates(
            &mut candidates,
            working_directory.join(requested_path),
            &extensions,
        );
    } else {
        searched_path = true;
        for entry in windows_search_path_entries(search_path) {
            let directory = match windows_search_path_entry_kind(&entry) {
                WindowsSearchPathEntryKind::Empty => working_directory.to_path_buf(),
                WindowsSearchPathEntryKind::DriveRelative => continue,
                WindowsSearchPathEntryKind::Absolute => entry,
                WindowsSearchPathEntryKind::Relative => working_directory.join(entry),
            };
            append_path_search_executable_candidates(
                &mut candidates,
                directory.join(requested_path),
                &extensions,
            );
        }
    }

    for candidate in candidates {
        match inspect(&candidate) {
            Ok(Some(resolved)) => return Ok(Some(resolved)),
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            // An unusable PATH entry does not make a later valid executable
            // unusable. This includes inaccessible directories, broken
            // network mappings, and canonicalization failures.
            Err(_) if searched_path => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to resolve Windows development command {}",
                        candidate.display()
                    )
                });
            }
        }
    }
    Ok(None)
}

fn is_drive_relative_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || !matches!(bytes[2], b'/' | b'\\'))
}

fn inspect_windows_executable_candidate(
    candidate: &Path,
) -> std::io::Result<Option<ResolvedWindowsExecutable>> {
    if !fs::metadata(candidate)?.is_file() {
        return Ok(None);
    }
    let canonical = fs::canonicalize(candidate)?;
    if !fs::metadata(&canonical)?.is_file() {
        return Ok(None);
    }
    Ok(Some(ResolvedWindowsExecutable {
        launch_path: candidate.to_path_buf(),
        canonical_path: canonical,
    }))
}

#[cfg(windows)]
fn command_shim_launch_path(path: &Path) -> io::Result<PathBuf> {
    // cmd.exe cannot dispatch verbatim paths. Keep this legacy conversion and
    // MAX_PATH boundary isolated to .cmd/.bat launch spellings; native PE
    // executables are passed to CreateProcessW by canonical/verbatim path.
    windows_command_compatible_path(path)
}

#[cfg(not(windows))]
fn command_shim_launch_path(path: &Path) -> io::Result<PathBuf> {
    Ok(path.to_path_buf())
}

#[cfg(windows)]
pub(crate) fn windows_command_compatible_path(path: &Path) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Ok(path.to_path_buf());
    };

    let mut compatible = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => {
            if !is_legacy_windows_path_component(server, false)
                || !is_legacy_windows_path_component(share, false)
            {
                return Err(incompatible_windows_path(path));
            }
            let mut prefix = OsString::from(r"\\");
            prefix.push(server);
            prefix.push(r"\");
            prefix.push(share);
            prefix.push(r"\");
            PathBuf::from(prefix)
        }
        Prefix::Verbatim(_) | Prefix::DeviceNS(_) => {
            return Err(incompatible_windows_path(path));
        }
        Prefix::Disk(_) | Prefix::UNC(_, _) => {
            if path.as_os_str().encode_wide().count() >= 260 {
                return Err(incompatible_windows_path(path));
            }
            return Ok(path.to_path_buf());
        }
    };

    for component in components {
        match component {
            Component::RootDir => {}
            Component::Normal(component) if is_legacy_windows_path_component(component, true) => {
                compatible.push(component);
            }
            _ => return Err(incompatible_windows_path(path)),
        }
    }

    // MAX_PATH includes the trailing NUL. A path that still needs the
    // verbatim namespace to fit must not be handed to cmd.exe as a batch path
    // or used as its child working directory.
    if compatible.as_os_str().encode_wide().count() >= 260 {
        return Err(incompatible_windows_path(path));
    }
    Ok(compatible)
}

#[cfg(windows)]
fn is_legacy_windows_path_component(component: &OsStr, reject_reserved: bool) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let wide = component.encode_wide().collect::<Vec<_>>();
    if wide.is_empty()
        || wide.len() > 255
        || wide.iter().any(|character| {
            *character <= 31
                || [b'<', b'>', b':', b'"', b'/', b'\\', b'|', b'?', b'*']
                    .into_iter()
                    .map(u16::from)
                    .any(|reserved| *character == reserved)
        })
        || matches!(wide.last(), Some(character) if *character == b' ' as u16 || *character == b'.' as u16)
    {
        return false;
    }
    if !reject_reserved {
        return true;
    }

    let stem_end = wide
        .iter()
        .position(|character| *character == b'.' as u16)
        .unwrap_or(wide.len());
    let stem = wide[..stem_end]
        .iter()
        .rposition(|character| *character != b' ' as u16 && *character != b'.' as u16)
        .map_or(&[][..], |end| &wide[..=end]);
    let reserved_base = ["CON", "PRN", "AUX", "NUL"]
        .into_iter()
        .any(|reserved| wide_eq_ignore_ascii_case(stem, reserved.as_bytes()));
    let reserved_numbered = stem.len() == 4
        && (wide_eq_ignore_ascii_case(&stem[..3], b"COM")
            || wide_eq_ignore_ascii_case(&stem[..3], b"LPT"))
        && (u16::from(b'1')..=u16::from(b'9')).contains(&stem[3]);
    !reserved_base && !reserved_numbered
}

#[cfg(windows)]
fn wide_eq_ignore_ascii_case(wide: &[u16], ascii: &[u8]) -> bool {
    wide.len() == ascii.len()
        && wide.iter().zip(ascii).all(|(wide, ascii)| {
            *wide <= u16::from(u8::MAX) && (*wide as u8).eq_ignore_ascii_case(ascii)
        })
}

#[cfg(windows)]
fn incompatible_windows_path(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "Windows verbatim path cannot be represented safely for cmd.exe: {}",
            path.display()
        ),
    )
}

#[cfg(windows)]
fn windows_search_path_entry_kind(entry: &Path) -> WindowsSearchPathEntryKind {
    use std::path::Component;

    if entry.as_os_str().is_empty() {
        return WindowsSearchPathEntryKind::Empty;
    }
    if matches!(entry.components().next(), Some(Component::Prefix(_))) && !entry.has_root() {
        return WindowsSearchPathEntryKind::DriveRelative;
    }
    if entry.is_absolute() {
        WindowsSearchPathEntryKind::Absolute
    } else {
        WindowsSearchPathEntryKind::Relative
    }
}

#[cfg(not(windows))]
fn windows_search_path_entry_kind(entry: &Path) -> WindowsSearchPathEntryKind {
    let bytes = entry.as_os_str().as_encoded_bytes();
    if bytes.is_empty() {
        return WindowsSearchPathEntryKind::Empty;
    }

    let drive_prefixed = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive_prefixed {
        return if bytes
            .get(2)
            .is_some_and(|separator| matches!(separator, b'/' | b'\\'))
        {
            WindowsSearchPathEntryKind::Absolute
        } else {
            WindowsSearchPathEntryKind::DriveRelative
        };
    }
    if bytes.len() >= 2 && matches!(bytes[0], b'/' | b'\\') && matches!(bytes[1], b'/' | b'\\') {
        return WindowsSearchPathEntryKind::Absolute;
    }
    if entry.is_absolute() {
        WindowsSearchPathEntryKind::Absolute
    } else {
        WindowsSearchPathEntryKind::Relative
    }
}

#[cfg(windows)]
fn windows_search_path_entries(search_path: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(search_path).collect()
}

#[cfg(all(test, unix, not(windows)))]
fn windows_search_path_entries(search_path: &OsStr) -> Vec<PathBuf> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let mut entries = Vec::new();
    let mut entry = Vec::new();
    let mut quoted = false;
    for byte in search_path.as_bytes() {
        match *byte {
            b'"' => quoted = !quoted,
            b';' if !quoted => {
                entries.push(PathBuf::from(OsString::from_vec(std::mem::take(
                    &mut entry,
                ))));
            }
            byte => entry.push(byte),
        }
    }
    entries.push(PathBuf::from(OsString::from_vec(entry)));
    entries
}

#[cfg(all(test, not(any(unix, windows))))]
fn windows_search_path_entries(search_path: &OsStr) -> Vec<PathBuf> {
    search_path
        .to_string_lossy()
        .split(';')
        .map(|entry| PathBuf::from(entry.trim_matches('"')))
        .collect()
}

fn executable_extensions(requested: &Path, configured: &OsStr) -> Vec<String> {
    if requested.extension().is_some() {
        return vec![String::new()];
    }
    let extensions = configured
        .to_string_lossy()
        .split(';')
        .filter(|extension| {
            extension.starts_with('.')
                && extension.len() > 1
                && extension[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric())
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    if extensions.is_empty() {
        DEFAULT_PATHEXT.split(';').map(str::to_string).collect()
    } else {
        extensions
    }
}

fn append_explicit_executable_candidates(
    candidates: &mut Vec<PathBuf>,
    base: PathBuf,
    extensions: &[String],
) {
    candidates.push(base.clone());
    for extension in extensions {
        if extension.is_empty() {
            continue;
        }
        let mut candidate = base.as_os_str().to_os_string();
        candidate.push(extension);
        candidates.push(PathBuf::from(candidate));
    }
}

fn append_path_search_executable_candidates(
    candidates: &mut Vec<PathBuf>,
    base: PathBuf,
    extensions: &[String],
) {
    for extension in extensions {
        let mut candidate = base.as_os_str().to_os_string();
        candidate.push(extension);
        candidates.push(PathBuf::from(candidate));
    }
}

fn is_command_shim(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "cmd" | "bat"))
}

fn resolve_command_interpreter(command_interpreter: Option<&OsStr>) -> Result<OsString> {
    resolve_command_interpreter_with(command_interpreter, || {
        #[cfg(windows)]
        {
            crate::windows_system::native_system_executable("cmd.exe")
        }
        #[cfg(not(windows))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "native Windows system-directory lookup is unavailable",
            ))
        }
    })
}

fn resolve_command_interpreter_with(
    command_interpreter: Option<&OsStr>,
    default: impl FnOnce() -> io::Result<PathBuf>,
) -> Result<OsString> {
    let command_interpreter = match command_interpreter {
        Some(command_interpreter) => command_interpreter.to_os_string(),
        None => default()
            .context("failed to resolve the native Windows cmd.exe")?
            .into_os_string(),
    };
    validate_command_interpreter(&command_interpreter)
}

fn validate_command_interpreter(command_interpreter: &OsStr) -> Result<OsString> {
    let text = command_interpreter
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Windows ComSpec must be valid Unicode"))?;
    if text.is_empty() || text.chars().any(char::is_control) || text.contains('"') {
        bail!("unsafe Windows ComSpec for development command");
    }
    if !is_absolute_windows_path(text) {
        bail!("Windows ComSpec must be an absolute executable path");
    }

    #[cfg(windows)]
    {
        let path = Path::new(command_interpreter);
        let resolved = inspect_windows_executable_candidate(path)
            .with_context(|| format!("failed to inspect Windows ComSpec {}", path.display()))?
            .ok_or_else(|| anyhow::anyhow!("Windows ComSpec is not a regular executable file"))?;
        return Ok(resolved.canonical_path.into_os_string());
    }
    #[cfg(not(windows))]
    {
        Ok(command_interpreter.to_os_string())
    }
}

fn is_absolute_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if drive_absolute {
        return true;
    }

    let normalized = path.replace('/', "\\");
    if normalized.starts_with(r"\\.\") {
        return false;
    }
    if let Some(verbatim) = normalized.strip_prefix(r"\\?\") {
        if let Some(unc) = verbatim.strip_prefix("UNC\\") {
            return unc.split('\\').filter(|part| !part.is_empty()).count() >= 2;
        }
        let bytes = verbatim.as_bytes();
        return bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\';
    }
    normalized
        .strip_prefix(r"\\")
        .is_some_and(|unc| unc.split('\\').filter(|part| !part.is_empty()).count() >= 2)
}

fn encode_command_shim_invocation(executable: &Path, arguments: &[String]) -> Result<String> {
    let executable = executable
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Windows command shim path must be valid Unicode"))?;
    if executable.is_empty()
        || has_windows_device_namespace_prefix(executable)
        || executable
            .chars()
            .any(|character| matches!(character, '\0' | '\r' | '\n' | '"'))
        || executable.ends_with('\\')
    {
        bail!("unsafe Windows development command shim path");
    }

    let mut command_line = String::from("\"");
    append_batch_argument_with_quote(&mut command_line, executable, true)?;
    for argument in arguments {
        command_line.push(' ');
        append_batch_argument(&mut command_line, argument)?;
    }
    command_line.push('"');
    Ok(command_line)
}

fn has_windows_device_namespace_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 4
        && matches!(bytes[0], b'/' | b'\\')
        && matches!(bytes[1], b'/' | b'\\')
        && matches!(bytes[2], b'?' | b'.')
        && matches!(bytes[3], b'/' | b'\\')
}

// Keep this aligned with Rust std's Windows `append_bat_arg`. Batch files are
// parsed first by cmd.exe and often forward `%*` to a native executable, so C
// argv quoting alone is insufficient. In particular, trailing backslashes must
// be doubled before a closing quote and literal percent signs must be protected
// from environment-variable expansion.
fn append_batch_argument(command_line: &mut String, argument: &str) -> Result<()> {
    append_batch_argument_with_quote(command_line, argument, false)
}

fn append_batch_argument_with_quote(
    command_line: &mut String,
    argument: &str,
    mut quote: bool,
) -> Result<()> {
    if argument
        .chars()
        .any(|character| matches!(character, '\0' | '\r' | '\n'))
    {
        bail!("Windows command-shim arguments must not contain NUL, CR, or LF");
    }

    const SAFE_UNQUOTED: &str = r"#$*+-./:?@\_";
    quote = quote
        || argument.is_empty()
        || argument.ends_with('\\')
        || argument.chars().any(|character| {
            let ascii_needs_quotes = character.is_ascii()
                && !(character.is_ascii_alphanumeric() || SAFE_UNQUOTED.contains(character));
            ascii_needs_quotes || character.is_control()
        });
    if quote {
        command_line.push('"');
    }

    let mut backslashes = 0usize;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
        } else {
            if character == '"' {
                command_line.extend(std::iter::repeat_n('\\', backslashes));
                command_line.push('"');
            } else if character == '%' {
                // `cd` is a built-in dynamic variable. Its zero-length slice
                // expands to nothing and prevents cmd.exe from interpreting the
                // user's percent sign as the start of `%NAME%` expansion.
                command_line.push_str("%%cd:~,");
            }
            backslashes = 0;
        }
        command_line.push(character);
    }
    if quote {
        command_line.extend(std::iter::repeat_n('\\', backslashes));
        command_line.push('"');
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn build_windows_app_command(
    argv: &[String],
    working_directory: &Path,
    environment_overrides: &[(String, String)],
) -> Result<std::process::Command> {
    use std::os::windows::process::CommandExt;

    let environment = WindowsEnvironment::effective(environment_overrides);
    match plan_windows_command(argv, working_directory, &environment)? {
        WindowsCommandPlan::Direct(program) => {
            let mut command = std::process::Command::new(program);
            command.args(&argv[1..]);
            Ok(command)
        }
        WindowsCommandPlan::CommandInterpreter {
            command_interpreter,
            command_line,
        } => {
            let mut command = std::process::Command::new(command_interpreter);
            command.args(["/d", "/e:on", "/s", "/v:off", "/c"]);
            command.raw_arg(command_line);
            Ok(command)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use crate::types::{AppKind, AppRunSpec, CommandSpec, ProxySettings};
    use tempfile::tempdir;

    fn environment(path: &Path, path_extensions: &str) -> WindowsEnvironment {
        WindowsEnvironment {
            path: path.as_os_str().to_os_string(),
            path_extensions: OsString::from(path_extensions),
            command_interpreter: Some(test_command_interpreter()),
        }
    }

    fn test_command_interpreter() -> OsString {
        #[cfg(windows)]
        {
            fs::canonicalize(crate::windows_system::native_system_executable("cmd.exe").unwrap())
                .unwrap()
                .into_os_string()
        }
        #[cfg(not(windows))]
        {
            OsString::from(r"C:\Windows\System32\cmd.exe")
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn missing_comspec_uses_injected_native_system_command_interpreter() {
        let command_interpreter = resolve_command_interpreter_with(None, || {
            Ok(PathBuf::from(r"D:\NativeWindows\System32\cmd.exe"))
        })
        .unwrap();

        assert_eq!(
            command_interpreter,
            OsString::from(r"D:\NativeWindows\System32\cmd.exe")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn missing_comspec_ignores_injected_cwd_and_path_cmd_shadows() {
        let temp = tempdir().unwrap();
        let working_directory = temp.path().join("working");
        let path_directory = temp.path().join("path");
        fs::create_dir_all(&working_directory).unwrap();
        fs::create_dir_all(&path_directory).unwrap();
        fs::write(path_directory.join("pnpm.CMD"), "shim").unwrap();
        fs::write(working_directory.join("cmd.exe"), "cwd shadow").unwrap();
        fs::write(path_directory.join("cmd.exe"), "PATH shadow").unwrap();
        let environment = WindowsEnvironment {
            path: path_directory.into_os_string(),
            path_extensions: OsString::from(".CMD"),
            command_interpreter: None,
        };
        let native = PathBuf::from(r"D:\NativeWindows\System32\cmd.exe");

        let plan = plan_windows_command_with_interpreter(
            &["pnpm".into(), "dev".into()],
            &working_directory,
            &environment,
            |command_interpreter| {
                resolve_command_interpreter_with(command_interpreter, || Ok(native.clone()))
            },
        )
        .unwrap();

        let WindowsCommandPlan::CommandInterpreter {
            command_interpreter,
            ..
        } = plan
        else {
            panic!("command shim was not dispatched through the native interpreter");
        };
        assert_eq!(command_interpreter, native.into_os_string());
    }

    #[test]
    fn missing_comspec_preserves_native_system_lookup_failure() {
        let error = resolve_command_interpreter_with(None, || {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected system-directory denial",
            ))
        })
        .unwrap_err();

        assert!(error.to_string().contains("native Windows cmd.exe"));
        assert!(
            error
                .chain()
                .any(|cause| cause.to_string() == "injected system-directory denial")
        );
    }

    #[test]
    fn drive_relative_windows_commands_are_rejected_before_candidate_inspection() {
        for requested in [r"C:node", r"c:tools\node.exe", "D:tools/node", "Z:"] {
            let error = resolve_windows_executable_with(
                OsStr::new(requested),
                Path::new(r"C:\repo"),
                OsStr::new(r"C:\tools"),
                OsStr::new(".EXE;.CMD"),
                |_| panic!("drive-relative commands must fail before candidate inspection"),
            )
            .unwrap_err();

            assert!(
                error.to_string().contains("drive-relative path"),
                "unexpected error for {requested:?}: {error:#}"
            );
        }
    }

    #[test]
    fn drive_relative_classifier_preserves_supported_command_forms() {
        for drive_relative in [r"C:node", r"c:tools\node.exe", "D:tools/node", "Z:"] {
            assert!(is_drive_relative_windows_path(drive_relative));
        }
        for supported in [
            "node",
            r".\node.exe",
            r"C:\node.exe",
            "C:/node.exe",
            r"\\server\share\node.exe",
            r"\node.exe",
        ] {
            assert!(
                !is_drive_relative_windows_path(supported),
                "supported command was classified as drive-relative: {supported:?}"
            );
        }
    }

    #[test]
    fn explicit_comspec_must_be_an_absolute_safe_windows_path() {
        for invalid in [
            "",
            "cmd.exe",
            r".\cmd.exe",
            r"C:cmd.exe",
            "C:\\Windows\\cmd.exe\nignored",
            "C:\\Windows\\\"cmd.exe",
            r"\\.\C:\Windows\System32\cmd.exe",
            r"\\?\relative\cmd.exe",
        ] {
            let error = resolve_command_interpreter_with(Some(OsStr::new(invalid)), || {
                panic!("an explicit ComSpec must not consult the native fallback")
            })
            .unwrap_err();
            assert!(
                error.to_string().contains("ComSpec"),
                "unexpected error for {invalid:?}: {error:#}"
            );
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn absolute_windows_comspec_forms_are_accepted_by_cross_target_validation() {
        for valid in [
            r"C:\Windows\System32\cmd.exe",
            r"C:/Windows/System32/cmd.exe",
            r"\\server\share\cmd.exe",
            r"\\?\C:\Windows\System32\cmd.exe",
            r"\\?\UNC\server\share\cmd.exe",
        ] {
            assert_eq!(
                resolve_command_interpreter_with(Some(OsStr::new(valid)), || unreachable!())
                    .unwrap(),
                OsString::from(valid)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn missing_comspec_resolves_to_native_system_cmd_on_windows() {
        let expected = crate::windows_system::native_system_executable("cmd.exe").unwrap();
        let resolved = resolve_command_interpreter(None).unwrap();

        assert_eq!(PathBuf::from(resolved), fs::canonicalize(expected).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn missing_comspec_ignores_native_cwd_and_path_cmd_shadows() {
        let temp = tempdir().unwrap();
        let working_directory = temp.path().join("working");
        let path_directory = temp.path().join("path");
        fs::create_dir_all(&working_directory).unwrap();
        fs::create_dir_all(&path_directory).unwrap();
        fs::write(path_directory.join("pnpm.CMD"), "@exit /b 0\r\n").unwrap();
        fs::write(working_directory.join("cmd.exe"), "cwd shadow").unwrap();
        fs::write(path_directory.join("cmd.exe"), "PATH shadow").unwrap();
        let environment = WindowsEnvironment {
            path: path_directory.into_os_string(),
            path_extensions: OsString::from(".CMD"),
            command_interpreter: None,
        };

        let plan = plan_windows_command(
            &["pnpm".into(), "dev".into()],
            &working_directory,
            &environment,
        )
        .unwrap();

        let WindowsCommandPlan::CommandInterpreter {
            command_interpreter,
            ..
        } = plan
        else {
            panic!("command shim was not dispatched through the native interpreter");
        };
        assert_eq!(command_interpreter, test_command_interpreter());
    }

    fn expected_native_launch_path(path: PathBuf) -> PathBuf {
        fs::canonicalize(path).unwrap()
    }

    fn expected_command_shim_path(path: PathBuf) -> PathBuf {
        command_shim_launch_path(&path).unwrap()
    }

    #[test]
    fn resolves_bare_command_shims_through_path_and_pathext() {
        let temp = tempdir().unwrap();
        let shim = temp.path().join("pnpm.CMD");
        fs::write(&shim, "shim").unwrap();

        let plan = plan_windows_command(
            &["pnpm".into(), "run".into(), "dev".into()],
            temp.path(),
            &environment(temp.path(), ".EXE;.CMD"),
        )
        .unwrap();

        let resolved = expected_command_shim_path(shim);
        assert_eq!(
            plan,
            WindowsCommandPlan::CommandInterpreter {
                command_interpreter: test_command_interpreter(),
                command_line: format!("\"\"{}\" run dev\"", resolved.display()),
            }
        );
    }

    #[test]
    fn native_executables_remain_direct() {
        let temp = tempdir().unwrap();
        let native = temp.path().join("node.EXE");
        fs::write(&native, "native").unwrap();

        let plan = plan_windows_command(
            &["node".into(), "server.js".into()],
            temp.path(),
            &environment(temp.path(), ".EXE;.CMD"),
        )
        .unwrap();

        assert_eq!(
            plan,
            WindowsCommandPlan::Direct(expected_native_launch_path(native).into_os_string())
        );
    }

    #[test]
    fn resolved_native_executable_uses_canonical_path_not_legacy_launch_spelling() {
        let canonical = PathBuf::from(format!(
            r"\\?\C:\{}\node.exe",
            "canonical-segment-".repeat(20)
        ));
        let plan = plan_resolved_windows_command(
            ResolvedWindowsExecutable {
                launch_path: PathBuf::from(r"C:\short-alias\node.exe"),
                canonical_path: canonical.clone(),
            },
            &["server.js".into()],
            &environment(Path::new(r"C:\tools"), ".EXE;.CMD"),
        )
        .unwrap();

        assert_eq!(plan, WindowsCommandPlan::Direct(canonical.into_os_string()));
    }

    #[test]
    fn resolved_command_shim_uses_legacy_launch_spelling_not_canonical_target() {
        let canonical = PathBuf::from(format!(
            r"\\?\C:\{}\pnpm.cmd",
            "canonical-segment-".repeat(20)
        ));
        let plan = plan_resolved_windows_command(
            ResolvedWindowsExecutable {
                launch_path: PathBuf::from(r"C:\short-alias\pnpm.cmd"),
                canonical_path: canonical.clone(),
            },
            &["dev".into()],
            &environment(Path::new(r"C:\tools"), ".EXE;.CMD"),
        )
        .unwrap();

        let WindowsCommandPlan::CommandInterpreter { command_line, .. } = plan else {
            panic!("command shim was not dispatched through cmd.exe");
        };
        assert!(command_line.contains(r"C:\short-alias\pnpm.cmd"));
        assert!(!command_line.contains(&canonical.display().to_string()));
    }

    #[test]
    fn explicit_extensionless_command_prefers_the_literal_file() {
        let temp = tempdir().unwrap();
        let literal = temp.path().join("astro");
        let shim = temp.path().join("astro.CMD");
        fs::write(&literal, "native").unwrap();
        fs::write(shim, "shim").unwrap();

        let plan = plan_windows_command(
            &[literal.display().to_string()],
            temp.path(),
            &environment(temp.path(), ".COM;.CMD"),
        )
        .unwrap();

        assert_eq!(
            plan,
            WindowsCommandPlan::Direct(expected_native_launch_path(literal).into_os_string())
        );
    }

    #[test]
    fn explicit_extensionless_command_skips_missing_pathext_candidates() {
        let temp = tempdir().unwrap();
        let requested = temp.path().join("astro");
        let shim = temp.path().join("astro.CMD");
        fs::write(&shim, "shim").unwrap();

        let plan = plan_windows_command(
            &[requested.display().to_string(), "dev".into()],
            temp.path(),
            &environment(temp.path(), ".COM;.EXE;.BAT;.CMD"),
        )
        .unwrap();

        let resolved = expected_command_shim_path(shim);
        assert_eq!(
            plan,
            WindowsCommandPlan::CommandInterpreter {
                command_interpreter: test_command_interpreter(),
                command_line: format!("\"\"{}\" dev\"", resolved.display()),
            }
        );
    }

    fn assert_actionable_unresolved_command_error(error: &anyhow::Error, requested: &str) {
        let message = error.to_string();
        assert!(
            message.contains(&format!("`{requested}`")),
            "resolution error did not identify the requested command: {error:#}"
        );
        assert!(
            message.contains("existing regular executable"),
            "resolution error did not explain the required executable: {error:#}"
        );
        assert!(
            message.contains(".cmd/.bat shim") && message.contains("PATH"),
            "resolution error did not provide actionable search guidance: {error:#}"
        );
    }

    #[test]
    fn skipped_only_drive_relative_path_fails_closed() {
        let temp = tempdir().unwrap();
        let environment = WindowsEnvironment {
            path: OsString::from("D:ambient"),
            path_extensions: OsString::from(".EXE;.CMD"),
            command_interpreter: Some(test_command_interpreter()),
        };

        let error = plan_windows_command(&["node".into()], temp.path(), &environment).unwrap_err();

        assert_actionable_unresolved_command_error(&error, "node");
    }

    #[test]
    fn skipped_drive_relative_and_missing_path_entries_fail_closed() {
        let temp = tempdir().unwrap();
        let environment = WindowsEnvironment {
            path: OsString::from("D:ambient;missing-tools"),
            path_extensions: OsString::from(".EXE"),
            command_interpreter: Some(test_command_interpreter()),
        };
        let mut inspected = Vec::new();
        let resolved = resolve_windows_executable_with(
            OsStr::new("node"),
            temp.path(),
            &environment.path,
            &environment.path_extensions,
            |candidate| {
                inspected.push(candidate.to_path_buf());
                Ok(None)
            },
        )
        .unwrap();

        assert_eq!(resolved, None);
        assert_eq!(
            inspected,
            vec![temp.path().join("missing-tools").join("node.EXE")]
        );

        let error = plan_windows_command(&["node".into()], temp.path(), &environment).unwrap_err();
        assert_actionable_unresolved_command_error(&error, "node");
    }

    #[test]
    fn missing_explicit_and_app_relative_commands_fail_closed() {
        let temp = tempdir().unwrap();
        let explicit = temp.path().join("missing.exe").display().to_string();
        let app_relative = "tools/missing";
        let environment = WindowsEnvironment {
            path: temp.path().as_os_str().to_os_string(),
            path_extensions: OsString::from(".EXE;.CMD"),
            command_interpreter: Some(test_command_interpreter()),
        };

        for requested in [&explicit, app_relative] {
            let error = plan_windows_command(&[requested.to_string()], temp.path(), &environment)
                .unwrap_err();
            assert_actionable_unresolved_command_error(&error, requested);
        }
    }

    #[test]
    fn path_search_skips_unusable_candidates_and_uses_later_executable() {
        let working_directory = Path::new("C:/repo");
        let path = OsStr::new("C:/denied;C:/dead-network;C:/tools");
        let mut inspected = Vec::new();
        let resolved = resolve_windows_executable_with(
            OsStr::new("node"),
            working_directory,
            path,
            OsStr::new(".EXE"),
            |candidate| {
                inspected.push(candidate.to_path_buf());
                let candidate = candidate.to_string_lossy();
                if candidate.contains("denied") {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "access denied",
                    ));
                }
                if candidate.contains("dead-network") {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "network path unavailable",
                    ));
                }
                Ok(candidate.contains("tools").then(|| {
                    let path = PathBuf::from(candidate.as_ref());
                    ResolvedWindowsExecutable {
                        launch_path: path.clone(),
                        canonical_path: path,
                    }
                }))
            },
        )
        .unwrap();

        assert!(
            resolved.as_ref().is_some_and(|resolved| resolved
                .launch_path
                .ends_with(Path::new("tools/node.EXE")))
        );
        assert_eq!(inspected.len(), 3);
    }

    #[test]
    fn path_search_skips_drive_relative_entries_and_preserves_candidate_order() {
        let working_directory = Path::new(r"C:\repo\app");
        let search_path =
            OsStr::new(r#"D:ambient;;tools;"C:\quoted;safe";C:\safe;\\server\share\bin"#);
        let mut inspected = Vec::new();

        let resolved = resolve_windows_executable_with(
            OsStr::new("node"),
            working_directory,
            search_path,
            OsStr::new(".EXE;.CMD"),
            |candidate| {
                inspected.push(candidate.to_path_buf());
                Ok(None)
            },
        )
        .unwrap();

        let expected = [
            working_directory.to_path_buf(),
            working_directory.join("tools"),
            PathBuf::from(r"C:\quoted;safe"),
            PathBuf::from(r"C:\safe"),
            PathBuf::from(r"\\server\share\bin"),
        ]
        .into_iter()
        .flat_map(|directory| {
            [".EXE", ".CMD"].map(move |extension| {
                let mut candidate = directory.join("node").into_os_string();
                candidate.push(extension);
                PathBuf::from(candidate)
            })
        })
        .collect::<Vec<_>>();

        assert_eq!(resolved, None);
        assert_eq!(inspected, expected);
    }

    #[cfg(windows)]
    #[test]
    fn native_path_search_never_inspects_drive_relative_candidates() {
        let expected = PathBuf::from(r"C:\safe\node.EXE");
        let mut inspected = Vec::new();

        let resolved = resolve_windows_executable_with(
            OsStr::new("node"),
            Path::new(r"C:\repo\app"),
            OsStr::new(r"D:ambient;C:\safe"),
            OsStr::new(".EXE"),
            |candidate| {
                inspected.push(candidate.to_path_buf());
                Ok((candidate == expected).then(|| ResolvedWindowsExecutable {
                    launch_path: candidate.to_path_buf(),
                    canonical_path: candidate.to_path_buf(),
                }))
            },
        )
        .unwrap();

        assert_eq!(
            resolved.map(|resolved| resolved.launch_path),
            Some(expected.clone())
        );
        assert_eq!(inspected, vec![expected]);
    }

    #[test]
    fn path_search_keeps_semicolons_inside_quoted_entries() {
        let working_directory = Path::new("C:/repo");
        let quoted_directory = PathBuf::from(r"C:\tools;beta");
        let expected_directory = quoted_directory;
        let expected = expected_directory.join("node.EXE");
        let mut inspected = Vec::new();

        let resolved = resolve_windows_executable_with(
            OsStr::new("node"),
            working_directory,
            OsStr::new(r#""C:\tools;beta";C:\fallback"#),
            OsStr::new(".EXE"),
            |candidate| {
                inspected.push(candidate.to_path_buf());
                Ok((candidate == expected).then(|| ResolvedWindowsExecutable {
                    launch_path: candidate.to_path_buf(),
                    canonical_path: candidate.to_path_buf(),
                }))
            },
        )
        .unwrap();

        assert_eq!(
            resolved
                .as_ref()
                .map(|resolved| resolved.launch_path.as_path()),
            Some(expected.as_path())
        );
        assert_eq!(inspected.first(), Some(&expected));
    }

    #[cfg(unix)]
    #[test]
    fn portable_windows_path_splitter_preserves_non_unicode_entries() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let first = b"relative-tools-\xff";
        let mut raw = first.to_vec();
        raw.extend_from_slice(b";C:/fallback");
        let search_path = OsString::from_vec(raw);
        let entries = windows_search_path_entries(&search_path);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].as_os_str().as_bytes(), first);
        assert_eq!(
            windows_search_path_entry_kind(&entries[0]),
            WindowsSearchPathEntryKind::Relative
        );

        let working_directory = Path::new("C:/repo");
        let mut inspected = Vec::new();
        resolve_windows_executable_with(
            OsStr::new("node"),
            working_directory,
            &search_path,
            OsStr::new(".EXE"),
            |candidate| {
                inspected.push(candidate.to_path_buf());
                Ok(None)
            },
        )
        .unwrap();
        let mut first_candidate = working_directory
            .join(&entries[0])
            .join("node")
            .into_os_string();
        first_candidate.push(".EXE");
        assert_eq!(inspected.first(), Some(&PathBuf::from(first_candidate)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_splitter_preserves_non_unicode_entries() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        let first = [
            u16::from(b'C'),
            u16::from(b':'),
            u16::from(b'\\'),
            u16::from(b't'),
            u16::from(b'o'),
            u16::from(b'o'),
            u16::from(b'l'),
            u16::from(b's'),
            u16::from(b'-'),
            0xd800,
        ];
        let mut raw = first.to_vec();
        raw.push(u16::from(b';'));
        raw.extend("C:\\fallback".encode_utf16());
        let entries = windows_search_path_entries(&OsString::from_wide(&raw));

        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].as_os_str().encode_wide().collect::<Vec<_>>(),
            first
        );
    }

    #[cfg(windows)]
    #[test]
    fn child_compatible_path_rejects_unsafe_verbatim_namespaces() {
        let unsafe_path = Path::new(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\repo");

        let error = windows_command_compatible_path(unsafe_path).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("cannot be represented safely"));
    }

    #[cfg(windows)]
    #[test]
    fn create_process_current_directory_keeps_legacy_max_path_boundary() {
        use std::os::windows::ffi::OsStrExt;

        let longest_supported = PathBuf::from(format!(r"C:\{}", "a".repeat(256)));
        assert_eq!(longest_supported.as_os_str().encode_wide().count(), 259);
        assert!(windows_command_compatible_path(&longest_supported).is_ok());

        let too_long = PathBuf::from(format!(r"C:\{}", "a".repeat(257)));
        assert_eq!(too_long.as_os_str().encode_wide().count(), 260);
        let error = windows_command_compatible_path(&too_long).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(windows)]
    #[test]
    fn long_batch_launch_path_is_rejected_without_restricting_native_target() {
        let long_tail = "a".repeat(270);
        let native = PathBuf::from(format!(r"\\?\C:\{long_tail}\node.exe"));
        let direct = plan_resolved_windows_command(
            ResolvedWindowsExecutable {
                launch_path: PathBuf::from(r"C:\short\node.exe"),
                canonical_path: native.clone(),
            },
            &[],
            &environment(Path::new(r"C:\tools"), ".EXE;.CMD"),
        )
        .unwrap();
        assert_eq!(direct, WindowsCommandPlan::Direct(native.into_os_string()));

        let batch = PathBuf::from(format!(r"\\?\C:\{long_tail}\pnpm.cmd"));
        let error = plan_resolved_windows_command(
            ResolvedWindowsExecutable {
                launch_path: batch.clone(),
                canonical_path: batch,
            },
            &[],
            &environment(Path::new(r"C:\tools"), ".EXE;.CMD"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot be represented safely"));
    }

    #[cfg(windows)]
    #[test]
    fn child_compatible_path_converts_verbatim_drive_and_unc_paths_at_launch_boundary() {
        assert_eq!(
            windows_command_compatible_path(Path::new(r"\\?\C:\repo\apps\web")).unwrap(),
            PathBuf::from(r"C:\repo\apps\web")
        );
        assert_eq!(
            windows_command_compatible_path(Path::new(r"\\?\UNC\server\share\repo\apps\web"))
                .unwrap(),
            PathBuf::from(r"\\server\share\repo\apps\web")
        );
    }

    #[test]
    fn explicit_windows_executable_preserves_inspection_errors() {
        let error = resolve_windows_executable_with(
            OsStr::new("C:/denied/node.exe"),
            Path::new("C:/repo"),
            OsStr::new("C:/tools"),
            OsStr::new(".EXE"),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "access denied",
                ))
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("C:/denied/node.exe"));
        assert!(
            error
                .chain()
                .any(|cause| cause.to_string() == "access denied")
        );
    }

    #[test]
    fn command_shim_encoder_preserves_batch_argv_semantics() {
        for (argument, expected) in [
            ("", "\"\""),
            ("simple", "simple"),
            ("two words", "\"two words\""),
            (r"C:\repo\", "\"C:\\repo\\\\\""),
            ("%PATH%", "\"%%cd:~,%PATH%%cd:~,%\""),
            ("a&b", "\"a&b\""),
            ("(group)", "\"(group)\""),
            ("say\"hi", "\"say\"\"hi\""),
            ("Zażółć", "Zażółć"),
        ] {
            let mut encoded = String::new();
            append_batch_argument(&mut encoded, argument).unwrap();
            assert_eq!(encoded, expected, "argument {argument:?}");
        }
    }

    #[test]
    fn command_shim_encoder_rejects_only_command_truncation_input() {
        for argument in ["nul\0byte", "line\nbreak", "line\rbreak"] {
            let mut encoded = String::new();
            let error = append_batch_argument(&mut encoded, argument).unwrap_err();
            assert!(error.to_string().contains("NUL, CR, or LF"));
        }
    }

    #[test]
    fn command_shim_path_uses_forced_batch_quoting_and_percent_protection() {
        let command_line = encode_command_shim_invocation(
            Path::new(r"C:\repo %name%\space & (group)\run.cmd"),
            &["dev".into()],
        )
        .unwrap();

        assert!(command_line.starts_with("\"\""));
        assert!(command_line.ends_with(" dev\""));
        assert!(command_line.contains("%%cd:~,%name%%cd:~,%"));
    }

    #[test]
    fn command_shim_encoder_rejects_remaining_verbatim_paths() {
        for executable in [
            Path::new(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\run.cmd"),
            Path::new(r"\\.\C:\run.cmd"),
            Path::new("//?/Volume{01234567-89ab-cdef-0123-456789abcdef}/run.cmd"),
        ] {
            let error = encode_command_shim_invocation(executable, &[]).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("unsafe Windows development command shim path")
            );
        }
    }

    #[test]
    fn effective_environment_overrides_are_case_insensitive() {
        let overrides = vec![
            ("Path".into(), r"C:\tools".into()),
            ("pathext".into(), ".CMD".into()),
            ("ComSpec".into(), r"D:\shell\cmd.exe".into()),
        ];
        let environment = WindowsEnvironment::effective(&overrides);
        assert_eq!(environment.path, OsString::from(r"C:\tools"));
        assert_eq!(environment.path_extensions, OsString::from(".CMD"));
        assert_eq!(
            environment.command_interpreter,
            Some(OsString::from(r"D:\shell\cmd.exe"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn command_shim_argv_roundtrip_helper() {
        if std::env::var_os("JIG_WINDOWS_BATCH_ROUNDTRIP").is_none() {
            return;
        }
        for (index, expected) in [
            "",
            "two words",
            r"C:\repo\",
            "%PATH%",
            "a&b",
            "(group)",
            "say\"hi",
            "Zażółć!",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                std::env::var(format!("JIG_WINDOWS_BATCH_ARG_{}", index + 1)).unwrap(),
                expected
            );
        }
        assert_eq!(std::env::var("ASTRO_DEV_BACKGROUND").unwrap(), "0");
    }

    #[cfg(windows)]
    #[test]
    fn command_shim_spawn_round_trips_complex_arguments() {
        let temp = tempdir().unwrap();
        let shim_dir = temp.path().join("% shim & (roundtrip)");
        fs::create_dir(&shim_dir).unwrap();
        let shim = shim_dir.join("jig-shim.CMD");
        let test_exe = std::env::current_exe()
            .unwrap()
            .display()
            .to_string()
            .replace('%', "%%");
        let mut script = String::from("@echo off\r\nsetlocal DisableDelayedExpansion\r\n");
        script.push_str("set \"JIG_WINDOWS_BATCH_ROUNDTRIP=1\"\r\n");
        for index in 1..=8 {
            script.push_str(&format!(
                "set \"JIG_WINDOWS_BATCH_ARG_{index}=%~{index}\"\r\n"
            ));
        }
        script.push_str(&format!(
            "@\"{test_exe}\" --exact processes::windows_launch::tests::command_shim_argv_roundtrip_helper --nocapture\r\n"
        ));
        fs::write(&shim, script).unwrap();
        let argv = vec![
            shim.display().to_string(),
            "".into(),
            "two words".into(),
            r"C:\repo\".into(),
            "%PATH%".into(),
            "a&b".into(),
            "(group)".into(),
            "say\"hi".into(),
            "Zażółć!".into(),
        ];

        let spec = AppRunSpec {
            name: "windows-shim".into(),
            dir: temp.path().to_path_buf(),
            command: CommandSpec::Argv(Vec::new()),
            kind: AppKind::EnvPort,
            hostname: "windows-shim.example.localhost".into(),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: false,
        };
        let settings = ProxySettings::default();
        let dev_env = [("ASTRO_DEV_BACKGROUND".to_string(), String::new())];
        let mut spawned = super::super::spawn_child(&spec, &argv, 4321, &settings, &dev_env)
            .expect("spawn encoded Windows command shim");
        let status = spawned.child.wait().unwrap();
        let output = String::from_utf8(spawned.output.captured_bytes()).unwrap();

        assert!(status.success(), "shim helper failed:\n{output}");
        super::super::child_lifecycle::terminate_and_reap(&mut spawned.child).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn exited_command_shim_descendants_remain_owned_by_the_app_job() {
        use std::time::{Duration, Instant};

        let temp = tempdir().unwrap();
        let shim = temp.path().join("background.CMD");
        fs::write(
            &shim,
            "@echo off\r\nstart \"\" /b \"%ComSpec%\" /d /s /c \"ping -n 30 127.0.0.1 >nul\"\r\nexit /b 0\r\n",
        )
        .unwrap();
        let argv = vec![shim.display().to_string()];
        let spec = AppRunSpec {
            name: "windows-background-shim".into(),
            dir: temp.path().to_path_buf(),
            command: CommandSpec::Argv(Vec::new()),
            kind: AppKind::EnvPort,
            hostname: "windows-background-shim.example.localhost".into(),
            target_host: "127.0.0.1".into(),
            explicit_port: None,
            proxy: false,
        };
        let mut spawned =
            super::super::spawn_child(&spec, &argv, 4321, &ProxySettings::default(), &[]).unwrap();
        let root_pid = spawned.child.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        while spawned.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            spawned.child.try_wait().unwrap().is_some(),
            "command shim wrapper did not exit"
        );
        assert!(
            super::super::child_lifecycle::windows_app_job_active_processes(root_pid)
                .unwrap()
                .is_some_and(|active| active > 0),
            "background descendant was not retained in the app job"
        );

        super::super::child_lifecycle::terminate_and_reap(&mut spawned.child).unwrap();

        assert_eq!(
            super::super::child_lifecycle::windows_app_job_active_processes(root_pid).unwrap(),
            None
        );
    }
}
