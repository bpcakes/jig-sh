use std::io;

const INITIAL_SYSTEM_DIRECTORY_CAPACITY: usize = 260;
const MAX_SYSTEM_DIRECTORY_CAPACITY: usize = 32_768;

fn validate_native_system_executable_name(name: &str) -> io::Result<()> {
    if name.is_empty()
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|character| character.is_control() || r#"/\:"<>|?*"#.contains(character))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows native system executable must be a plain file name",
        ));
    }
    Ok(())
}

fn system_directory_wide_with(
    mut query: impl FnMut(&mut [u16]) -> io::Result<usize>,
) -> io::Result<Vec<u16>> {
    let mut buffer = vec![0; INITIAL_SYSTEM_DIRECTORY_CAPACITY];
    loop {
        let length = query(&mut buffer)?;
        if length < buffer.len() {
            buffer.truncate(length);
            if buffer.is_empty() || buffer.contains(&0) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Windows system directory was empty or contained an embedded NUL",
                ));
            }
            return Ok(buffer);
        }

        let required = length.max(buffer.len().saturating_add(1));
        if required > MAX_SYSTEM_DIRECTORY_CAPACITY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows system directory exceeded the supported path length",
            ));
        }
        buffer.resize(required, 0);
    }
}

#[cfg(windows)]
pub(crate) fn native_system_executable(name: &str) -> io::Result<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    validate_native_system_executable_name(name)?;

    let directory = system_directory_wide_with(|buffer| {
        let length = unsafe {
            // SAFETY: `buffer` is writable UTF-16 storage and its exact capacity
            // is passed to GetSystemDirectoryW.
            windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "system path buffer was too large",
                    )
                })?,
            )
        };
        if length == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(length as usize)
        }
    })?;
    Ok(std::path::PathBuf::from(OsString::from_wide(&directory)).join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn system_directory_query_resizes_and_excludes_the_trailing_nul() {
        let calls = Cell::new(0usize);
        let expected = "D:\\NativeWindows\\System32"
            .encode_utf16()
            .collect::<Vec<_>>();
        let actual = system_directory_wide_with(|buffer| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                return Ok(buffer.len() + 17);
            }
            buffer[..expected.len()].copy_from_slice(&expected);
            buffer[expected.len()] = 0;
            Ok(expected.len())
        })
        .unwrap();

        assert_eq!(actual, expected);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn system_directory_query_rejects_empty_embedded_nul_and_unbounded_results() {
        assert!(system_directory_wide_with(|_| Ok(0)).is_err());
        assert!(
            system_directory_wide_with(|buffer| {
                buffer[0] = u16::from(b'C');
                buffer[1] = 0;
                Ok(2)
            })
            .is_err()
        );
        assert!(system_directory_wide_with(|_| Ok(MAX_SYSTEM_DIRECTORY_CAPACITY + 1)).is_err());
    }

    #[test]
    fn native_system_executable_name_rejects_path_and_device_syntax() {
        for invalid in [
            "",
            ".",
            "..",
            r"..\cmd.exe",
            "subdir/cmd.exe",
            r"C:\cmd.exe",
            "taskkill.exe:stream",
            "cmd.exe\nignored",
        ] {
            assert!(
                validate_native_system_executable_name(invalid).is_err(),
                "unexpectedly accepted {invalid:?}"
            );
        }
        for valid in ["cmd.exe", "taskkill.exe", "icacls.exe"] {
            validate_native_system_executable_name(valid).unwrap();
        }
    }
}
