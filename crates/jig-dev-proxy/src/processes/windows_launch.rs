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
mod tests;
