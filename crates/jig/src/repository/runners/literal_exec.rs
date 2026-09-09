//! Prevent libc's execvp ENOEXEC fallback from implicitly invoking /bin/sh.
//! Only used for argv runners, which always inherit the environment.

use std::process::Command;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn prepare(command: &mut Command) -> std::io::Result<()> {
    use std::{
        collections::BTreeMap,
        ffi::CString,
        os::unix::{ffi::OsStrExt, process::CommandExt},
        path::Path,
    };

    let mut environment = std::env::vars_os().collect::<BTreeMap<_, _>>();
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            environment.insert(key.to_owned(), value.to_owned());
        } else {
            environment.remove(key);
        }
    }
    let working_directory = command
        .get_current_dir()
        .map(Path::to_owned)
        .map_or_else(std::env::current_dir, Ok)?;
    let program = command.get_program();
    let path_search = !program.as_bytes().contains(&b'/');
    let candidates = if !path_search {
        vec![working_directory.join(program)]
    } else {
        let path = environment.get(std::ffi::OsStr::new("PATH")).map_or_else(
            || std::ffi::OsStr::new(super::DEFAULT_ARGV_SEARCH_PATH),
            |value| value.as_os_str(),
        );
        std::env::split_paths(path)
            .map(|directory| working_directory.join(directory).join(program))
            .collect()
    };
    let c_string = |bytes: &[u8]| {
        CString::new(bytes).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "literal process input contains NUL",
            )
        })
    };
    let paths = candidates
        .iter()
        .map(|path| c_string(path.as_os_str().as_bytes()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let argv = std::iter::once(program)
        .chain(command.get_args())
        .map(|value| c_string(value.as_bytes()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let env = environment
        .iter()
        .map(|(key, value)| {
            let mut bytes = key.as_bytes().to_vec();
            bytes.push(b'=');
            bytes.extend_from_slice(value.as_bytes());
            c_string(&bytes)
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let input = ExecInput::new(paths, argv, env, path_search);
    // SAFETY: all allocation and environment access happens above, before fork.
    // The callback uses only immutable owned C strings, execve, and errno access.
    // It either replaces the child or returns a raw OS error; it never falls
    // through to std's execvp, which may interpret an ENOEXEC file as shell text.
    unsafe {
        command.pre_exec(move || input.execute());
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
struct ExecInput {
    paths: Vec<std::ffi::CString>,
    path_search: bool,
    // These buffers own every pointee in the corresponding pointer arrays.
    _argv: Vec<std::ffi::CString>,
    _env: Vec<std::ffi::CString>,
    argv_ptrs: Vec<*const libc::c_char>,
    env_ptrs: Vec<*const libc::c_char>,
}

// SAFETY: pointers refer exclusively to this object's immutable CString heap
// buffers. Moving the owner does not move those buffers; nothing mutates them.
#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe impl Send for ExecInput {}
// SAFETY: execute only reads the immutable buffers and pointer arrays.
#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe impl Sync for ExecInput {}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl ExecInput {
    fn new(
        paths: Vec<std::ffi::CString>,
        argv: Vec<std::ffi::CString>,
        env: Vec<std::ffi::CString>,
        path_search: bool,
    ) -> Self {
        let pointers = |values: &[std::ffi::CString]| {
            values
                .iter()
                .map(|value| value.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect()
        };
        Self {
            paths,
            path_search,
            argv_ptrs: pointers(&argv),
            env_ptrs: pointers(&env),
            _argv: argv,
            _env: env,
        }
    }

    fn execute(&self) -> std::io::Result<()> {
        self.execute_with(|path| {
            // SAFETY: path and every argv/env element are live NUL-terminated
            // strings; both pointer arrays have a terminating null pointer.
            unsafe {
                libc::execve(
                    path.as_ptr(),
                    self.argv_ptrs.as_ptr(),
                    self.env_ptrs.as_ptr(),
                );
            }
            std::io::Error::last_os_error()
        })
    }

    // Inject only the syscall result in tests; the production callback above
    // never returns on success. Both paths use the same allocation-free walk.
    fn execute_with(
        &self,
        mut attempt: impl FnMut(&std::ffi::CStr) -> std::io::Error,
    ) -> std::io::Result<()> {
        let mut denied = false;
        for path in &self.paths {
            let error = attempt(path);
            if !self.path_search {
                return Err(error);
            }
            match error.raw_os_error() {
                Some(libc::EACCES) => denied = true,
                Some(libc::ENOENT | libc::ENOTDIR) => {}
                // glibc treats these filesystem errors as unusable PATH entries.
                #[cfg(target_os = "linux")]
                Some(libc::ESTALE | libc::ENODEV | libc::ETIMEDOUT) => {}
                // Match Darwin execvp's PATH traversal without its ENOEXEC
                // shell fallback. Linux treats these errors as fatal.
                #[cfg(target_os = "macos")]
                Some(libc::ELOOP | libc::ENAMETOOLONG) => {}
                _ => return Err(error),
            }
        }
        Err(std::io::Error::from_raw_os_error(if denied {
            libc::EACCES
        } else {
            libc::ENOENT
        }))
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn prepare(_command: &mut Command) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "literal argv supervision is unavailable on this platform",
    ))
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    fn fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("bin")).unwrap();
        fs::copy("/usr/bin/true", temp.path().join("bin/example-tool")).unwrap();
        temp
    }

    fn run(root: &std::path::Path, path: &str) -> std::io::Result<std::process::ExitStatus> {
        let mut command = Command::new("example-tool");
        command.current_dir(root).env("PATH", path);
        prepare(&mut command)?;
        command.status()
    }

    #[test]
    fn literal_exec_path_search_skips_missing_and_denied_candidates() {
        let temp = fixture();
        fs::create_dir(temp.path().join("denied")).unwrap();
        let denied = temp.path().join("denied/example-tool");
        fs::write(&denied, "not executable").unwrap();
        fs::set_permissions(denied, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(run(temp.path(), "missing:denied:bin").unwrap().success());
        assert!(run(&temp.path().join("bin"), "").unwrap().success());
        assert_eq!(
            run(temp.path(), "denied:missing")
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EACCES)
        );
        assert_eq!(
            run(temp.path(), "missing").unwrap_err().raw_os_error(),
            Some(libc::ENOENT)
        );
    }

    #[test]
    fn literal_exec_path_errors_follow_platform_search_behavior() {
        let temp = fixture();
        symlink("loop", temp.path().join("loop")).unwrap();
        for (entry, errno) in [
            ("loop".to_string(), libc::ELOOP),
            ("x".repeat(300), libc::ENAMETOOLONG),
        ] {
            let result = run(temp.path(), &format!("{entry}:bin"));
            if cfg!(target_os = "macos") {
                assert!(result.unwrap().success());
            } else {
                assert_eq!(result.unwrap_err().raw_os_error(), Some(errno));
            }
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn literal_exec_skips_unreachable_path_entries_but_keeps_explicit_errors() {
        for errno in [libc::ESTALE, libc::ENODEV, libc::ETIMEDOUT] {
            for path_search in [true, false] {
                let input = ExecInput::new(
                    ["unreachable/example-tool", "available/example-tool"]
                        .map(|path| std::ffi::CString::new(path).unwrap())
                        .to_vec(),
                    vec![],
                    vec![],
                    path_search,
                );
                let mut attempted = Vec::new();
                let error = input
                    .execute_with(|path| {
                        attempted.push(path.to_str().unwrap().to_owned());
                        // ENOEXEC on the second candidate proves we reached it and
                        // still fail without attempting an implicit shell fallback.
                        std::io::Error::from_raw_os_error(if attempted.len() == 1 {
                            errno
                        } else {
                            libc::ENOEXEC
                        })
                    })
                    .unwrap_err();
                if path_search {
                    assert_eq!(
                        attempted,
                        ["unreachable/example-tool", "available/example-tool"]
                    );
                    assert_eq!(error.raw_os_error(), Some(libc::ENOEXEC));
                } else {
                    assert_eq!(attempted, ["unreachable/example-tool"]);
                    assert_eq!(error.raw_os_error(), Some(errno));
                }
            }
        }
    }
}
