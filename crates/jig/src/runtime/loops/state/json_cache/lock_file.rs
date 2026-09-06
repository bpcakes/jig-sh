const LOCK_FILE_CREATE_RETRIES: usize = 8;

fn open_or_create_lock_file(directory: &Dir, name: &OsStr, path: &Path) -> Result<File> {
    // Separate existing opens from exclusive creation so concurrent cold starts
    // converge on one inode without relying on a contended O_CREAT open.
    for _ in 0..LOCK_FILE_CREATE_RETRIES {
        match open_regular_file(directory, name, true, false, false, path) {
            Ok(file) => return Ok(file),
            Err(error) if error_has_io_kind(&error, io::ErrorKind::NotFound) => {}
            Err(error) => return Err(error),
        }
        match open_regular_file(directory, name, true, false, true, path) {
            Ok(file) => return Ok(file),
            Err(error)
                if error_has_io_kind(&error, io::ErrorKind::AlreadyExists)
                    || error_has_io_kind(&error, io::ErrorKind::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    bail!(
        "Loop cache lock path changed repeatedly while opening {}",
        path.display()
    )
}

fn open_optional_regular_file(directory: &Dir, name: &OsStr, path: &Path) -> Result<Option<File>> {
    match open_regular_file(directory, name, false, false, false, path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error_has_io_kind(&error, io::ErrorKind::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

fn open_regular_file(
    directory: &Dir,
    name: &OsStr,
    writable: bool,
    create: bool,
    create_new: bool,
    path: &Path,
) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(!create_new)
        .write(writable)
        .create(create)
        .create_new(create_new)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = directory
        .open_with(name, &options)
        .map(cap_std::fs::File::into_std)
        .with_context(|| {
            format!(
                "Failed to open loop cache file {} without following links",
                path.display()
            )
        })?;
    if !file.metadata()?.is_file() {
        bail!("Loop cache path is not a regular file: {}", path.display());
    }
    Ok(file)
}

fn error_has_io_kind(error: &anyhow::Error, expected: io::ErrorKind) -> bool {
    error
        .downcast_ref::<io::Error>()
        .is_some_and(|error| error.kind() == expected)
}
