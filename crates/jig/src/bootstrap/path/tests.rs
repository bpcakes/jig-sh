use super::*;
use tempfile::tempdir;

#[test]
fn init_destination_normalizes_current_components_and_rejects_parent_components() {
    let base = tempdir().unwrap();
    let base = fs::canonicalize(base.path()).unwrap();

    assert_eq!(
        resolve_init_destination(Path::new("."), &base).unwrap(),
        base
    );
    assert_eq!(
        resolve_init_destination(Path::new("./nested//./repo"), &base).unwrap(),
        base.join("nested/repo")
    );

    for path in ["..", "missing/../repo", "./nested/../../repo"] {
        let error = resolve_init_destination(Path::new(path), &base)
            .unwrap_err()
            .to_string();
        assert!(error.contains("must not contain '..'"), "{path}: {error}");
        assert!(!base.join("missing").exists());
        assert!(!base.join("nested").exists());
    }
}

#[cfg(unix)]
#[test]
fn init_destination_canonicalizes_only_existing_ancestors_of_missing_tails() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let base = fs::canonicalize(temp.path()).unwrap();
    let first = base.join("first");
    let second = base.join("second");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let link = base.join("link");
    symlink(&first, &link).unwrap();

    assert_eq!(
        resolve_init_destination(&link, &base).unwrap(),
        link,
        "an existing final symlink must remain visible to destination validation"
    );
    let resolved = resolve_init_destination(&link.join("nested/repo"), &base).unwrap();
    assert_eq!(resolved, first.join("nested/repo"));

    fs::remove_file(&link).unwrap();
    symlink(&second, &link).unwrap();
    assert_eq!(
        resolved,
        first.join("nested/repo"),
        "retargeting the spelling after resolution must not redirect init"
    );
}

#[cfg(windows)]
#[test]
fn init_destination_rejects_incomplete_windows_absolute_forms() {
    let base = tempdir().unwrap();
    let base = fs::canonicalize(base.path()).unwrap();
    for path in [r"C:repo", r"\repo"] {
        let error = resolve_init_destination(Path::new(path), &base)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("complete absolute drive/UNC"),
            "{path}: {error}"
        );
    }
}

#[test]
fn portable_planned_files_reject_component_prefix_and_ascii_case_collisions() {
    for paths in [
        [
            Path::new("package.json"),
            Path::new("package.json/app.json"),
        ],
        [Path::new("Web/app.json"), Path::new("web/app.json")],
    ] {
        let error = validate_portable_planned_file_collisions(paths)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Portable planned repository file collision"));
        for path in paths {
            assert!(error.contains(&path.display().to_string()), "{error}");
        }
    }
}

#[test]
fn portable_collision_validation_scales_to_large_plans() {
    let mut paths = (0..50_000)
        .map(|index| PathBuf::from(format!("generated/{index:05}.txt")))
        .collect::<Vec<_>>();
    validate_portable_planned_file_collisions(&paths).unwrap();
    paths.push(PathBuf::from("GENERATED/49999.TXT"));
    let error = validate_portable_planned_file_collisions(&paths)
        .unwrap_err()
        .to_string();
    assert!(error.contains("generated/49999.txt"), "{error}");
    assert!(error.contains("GENERATED/49999.TXT"), "{error}");
}

#[test]
fn file_fingerprints_reject_same_inode_in_place_mutation() {
    let root = tempdir().unwrap();
    let path = root.path().join("state");
    fs::write(&path, b"before-state").unwrap();
    let before = repository_file_fingerprint_at(&path).unwrap();
    fs::write(&path, b"after--state").unwrap();
    let after = repository_file_fingerprint_at(&path).unwrap();
    assert_eq!(before.identity, after.identity);
    assert!(!repository_file_commits_match(&before, &after));
}

#[cfg(unix)]
#[test]
fn retained_directory_and_symlink_handles_reject_recreated_paths() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let directory = root.path().join("directory");
    let retained_directory = root.path().join("retained-directory");
    fs::create_dir(&directory).unwrap();
    let directory_commit = repository_directory_commit_at(&directory).unwrap();
    fs::rename(&directory, &retained_directory).unwrap();
    fs::create_dir(&directory).unwrap();
    assert!(!repository_directory_commit_matches_path(&directory_commit, &directory).unwrap());

    let link = root.path().join("link");
    let retained_link = root.path().join("retained-link");
    symlink("first", &link).unwrap();
    let link_commit = repository_symlink_commit_at(&link).unwrap();
    fs::rename(&link, &retained_link).unwrap();
    symlink("first", &link).unwrap();
    assert_ne!(
        repository_path_identity(&link).unwrap(),
        link_commit.identity
    );
    assert_eq!(
        repository_file_identity(&link_commit.handle).unwrap(),
        link_commit.identity
    );
}

#[cfg(windows)]
#[test]
fn windows_real_directory_predicate_rejects_reparse_points() {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    assert!(windows_directory_attributes_are_real(true, false, 0));
    assert!(!windows_directory_attributes_are_real(
        true,
        false,
        FILE_ATTRIBUTE_REPARSE_POINT,
    ));
    assert!(!windows_directory_attributes_are_real(false, false, 0));
    assert!(!windows_directory_attributes_are_real(true, true, 0));
}

#[test]
fn portable_planned_files_reject_windows_aliases_and_devices() {
    for path in [
        "web./app.json",
        "CON/app.json",
        "prn.txt/app.json",
        "AUX/app.json",
        "nul.json/app.json",
        "COM1/app.json",
        "com9.log/app.json",
        "LPT1/app.json",
        "lpt9.txt/app.json",
        "COM¹/app.json",
        "com².txt/app.json",
        "LPT³/app.json",
    ] {
        let error = validate_portable_planned_file_collisions([Path::new(path)])
            .unwrap_err()
            .to_string();
        assert!(error.contains("not portable to Windows"), "{path}: {error}");
        assert!(error.contains(path), "{path}: {error}");
    }

    validate_portable_planned_file_collisions([
        Path::new("console/app.json"),
        Path::new("com0/app.json"),
        Path::new("com10/app.json"),
        Path::new("lpt0/app.json"),
        Path::new("lpt10/app.json"),
        Path::new("com⁰/app.json"),
        Path::new("com⁴/app.json"),
        Path::new("lpt⁰/app.json"),
        Path::new("lpt⁴/app.json"),
    ])
    .unwrap();
}

#[test]
fn portable_planned_files_reject_windows_forbidden_characters_and_controls() {
    for character in ['<', '>', ':', '"', '|', '?', '*'] {
        let path = format!("nested/bad{character}name.txt");
        let error = validate_portable_planned_file_collisions([Path::new(&path)])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not portable to Windows"),
            "{path:?}: {error}"
        );
        assert!(error.contains("forbidden character"), "{path:?}: {error}");
    }

    for byte in (0_u8..=31).chain(std::iter::once(127)) {
        let path = format!("nested/bad{}name.txt", char::from(byte));
        let error = validate_portable_planned_file_collisions([Path::new(&path)])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not portable to Windows"),
            "0x{byte:02x}: {error}"
        );
        assert!(error.contains("control byte"), "0x{byte:02x}: {error}");
    }

    validate_portable_planned_file_collisions([
        Path::new("nested/good+name.txt"),
        Path::new("nested/good,name.txt"),
        Path::new("nested/good;name.txt"),
        Path::new("nested/good[name].txt"),
    ])
    .unwrap();
}

#[cfg(unix)]
#[test]
fn portable_planned_files_reject_raw_backslash_components() {
    let backslash = Path::new(r"nested\bad\name.txt");
    let error = validate_portable_planned_file_collisions([backslash])
        .unwrap_err()
        .to_string();
    assert!(error.contains("raw backslash"), "{error}");
}

#[cfg(unix)]
#[test]
fn portable_planned_files_reject_non_unicode_components() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let mut path = PathBuf::from("nested");
    path.push(OsString::from_vec(b"bad-\xff-name.txt".to_vec()));

    let error = validate_portable_planned_file_collisions([&path])
        .unwrap_err()
        .to_string();

    assert!(error.contains("valid Unicode"), "{error}");
    validate_portable_planned_file_collisions([Path::new("nested/Zażółć.txt")]).unwrap();
}

#[test]
fn atomic_noreplace_capability_probe_uses_the_destination_filesystem_and_cleans_up() {
    let parent = tempdir().unwrap();

    ensure_atomic_noreplace_publication_supported(parent.path()).unwrap();

    let leftovers = fs::read_dir(parent.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert!(
        leftovers.is_empty(),
        "capability probe leaked: {leftovers:?}"
    );
}

#[test]
fn unsupported_atomic_noreplace_probe_preserves_its_unmodified_artifact() {
    let parent = tempdir().unwrap();

    let error = ensure_atomic_noreplace_publication_supported_with(
        parent.path(),
        |_source, _destination| {
            Err(io::Error::new(
                ErrorKind::Unsupported,
                "injected unsupported rename",
            ))
        },
    )
    .unwrap_err();

    let message = format!("{error:#}");
    assert!(
        message.contains("atomic no-replace directory rename"),
        "{message}"
    );
    assert!(
        message.contains("preserving the capability probe"),
        "{message}"
    );
    let probes = fs::read_dir(parent.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(probes.len(), 1, "unexpected probe artifacts: {probes:?}");
    assert!(
        probes[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(".jig-noreplace-probe-")
    );
    assert!(probes[0].join("source").is_dir());
    assert!(probes[0].join("occupied-destination").is_dir());
    assert!(!probes[0].join("published-destination").exists());
}

#[cfg(unix)]
#[test]
fn temporary_symlink_cleanup_quarantines_and_removes_the_retained_entry() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().unwrap();
    let temporary = parent.path().join("temporary-link");
    symlink("owned-target", &temporary).unwrap();
    let commit = repository_symlink_commit_at(&temporary).unwrap();

    let error = cleanup_temporary_symlink(
        &temporary,
        &commit.identity,
        anyhow::anyhow!("injected publication failure"),
    );

    assert!(format!("{error:#}").contains("injected publication failure"));
    assert!(
        fs::symlink_metadata(&temporary).is_err_and(|error| error.kind() == ErrorKind::NotFound)
    );
    assert!(fs::read_dir(parent.path()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn temporary_symlink_cleanup_preserves_a_foreign_quarantine_replacement() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().unwrap();
    let temporary = parent.path().join("temporary-link");
    let displaced_owned = parent.path().join("displaced-owned-link");
    symlink("owned-target", &temporary).unwrap();
    let commit = repository_symlink_commit_at(&temporary).unwrap();

    let error = cleanup_temporary_symlink_with(
        &temporary,
        &commit.identity,
        anyhow::anyhow!("injected publication failure"),
        |quarantine| {
            fs::rename(quarantine, &displaced_owned).unwrap();
            symlink("foreign-target", quarantine).unwrap();
        },
    );

    let message = format!("{error:#}");
    assert!(
        message.contains("refusing to unlink the replacement"),
        "{message}"
    );
    assert!(message.contains("Restored the changed entry"), "{message}");
    assert_eq!(
        fs::read_link(&temporary).unwrap(),
        Path::new("foreign-target")
    );
    assert_eq!(
        fs::read_link(&displaced_owned).unwrap(),
        Path::new("owned-target")
    );
}

#[test]
fn repository_relative_ancestor_validation_allows_directories_and_missing_ancestors() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("existing")).unwrap();

    validate_repository_relative_ancestors(root.path(), Path::new("existing/file")).unwrap();
    validate_repository_relative_ancestors(root.path(), Path::new("missing/deep/file")).unwrap();
}

#[test]
fn repository_relative_ancestor_validation_rejects_escaping_paths() {
    let root = tempdir().unwrap();

    for relative in [Path::new("../outside"), root.path()] {
        let error = validate_repository_relative_ancestors(root.path(), relative)
            .unwrap_err()
            .to_string();
        assert!(error.contains("contained relative path"), "{error}");
    }
}

#[test]
fn repository_relative_path_validation_rejects_reserved_git_metadata_aliases() {
    for relative in [
        ".git",
        ".git.",
        ".git ",
        "vendor/.GiT.../config",
        "vendor/.GIT. . /config",
        "GIT~1/config",
        "vendor/git~1. . /config",
        ".git:stream",
        ".git .:stream",
        ".git::$INDEX_ALLOCATION",
        ".git...:alternate-stream",
        "git~1::$DATA",
        ".g\u{200c}it/config",
        "\u{feff}.G\u{202e}i\u{206a}T/config",
        "vendor\\.GiT...\\config",
    ] {
        let error = validate_no_reserved_git_metadata_components(Path::new(relative))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("reserved Git metadata component"),
            "{relative}: {error}"
        );
        assert!(error.contains(relative), "{relative}: {error}");
    }
}

#[test]
fn repository_relative_path_validation_allows_git_near_misses() {
    for relative in [
        ".github/workflows/check.yml",
        ".gitignore",
        ".gitkeep",
        "git/config",
        "git~2/config",
        "git~10/config",
        "git~1x/config",
        ".gitx. ",
        ".gitx:stream",
        ".git .config",
        ".git\u{a0}",
        ".git\u{200b}",
        ".gi\u{200b}t",
        ".git\u{2029}",
        ".git\u{2060}",
        ".git\u{2069}",
        ".g\u{200c}itx",
    ] {
        validate_no_reserved_git_metadata_components(Path::new(relative)).unwrap();
    }
}

#[test]
fn repository_relative_path_validation_ignores_only_git_hfs_codepoints() {
    for ignored in [
        '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}',
        '\u{202d}', '\u{202e}', '\u{206a}', '\u{206b}', '\u{206c}', '\u{206d}', '\u{206e}',
        '\u{206f}', '\u{feff}',
    ] {
        let relative = format!(".g{ignored}it/config");
        validate_no_reserved_git_metadata_components(Path::new(&relative)).unwrap_err();
    }
}

#[test]
fn repository_relative_ancestor_validation_rejects_non_directory_ancestors() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("blocking"), "file").unwrap();

    let error = validate_repository_relative_ancestors(root.path(), Path::new("blocking/file"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not a directory"), "{error}");
}

#[cfg(unix)]
#[test]
fn repository_relative_ancestor_validation_rejects_symlink_ancestors() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("target")).unwrap();
    symlink("target", root.path().join("linked")).unwrap();

    let error = validate_repository_relative_ancestors(root.path(), Path::new("linked/file"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is a symlink"), "{error}");
}

#[test]
fn repository_relative_ancestor_validation_requires_a_real_directory_root() {
    let parent = tempdir().unwrap();
    let file_root = parent.path().join("file-root");
    fs::write(&file_root, "file").unwrap();

    let error = validate_repository_relative_ancestors(&file_root, Path::new("child"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("real directory"), "{error}");
}

#[test]
fn repository_relative_file_leaf_validation_classifies_files_and_missing_paths() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file"), "contents").unwrap();

    assert_eq!(
        validate_repository_relative_file_leaf(root.path(), Path::new("file")).unwrap(),
        RepositoryFileLeaf::RegularFile
    );
    assert_eq!(
        validate_repository_relative_file_leaf(root.path(), Path::new("missing")).unwrap(),
        RepositoryFileLeaf::Missing
    );
}

#[test]
fn repository_relative_file_leaf_validation_rejects_directories() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("directory")).unwrap();

    let error = validate_repository_relative_file_leaf(root.path(), Path::new("directory"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("destination leaf"), "{error}");
    assert!(error.contains("is a directory"), "{error}");
}

#[test]
fn atomic_repository_write_failure_leaves_the_existing_file_unchanged() {
    let root = tempdir().unwrap();
    let relative = Path::new("managed.txt");
    fs::write(root.path().join(relative), b"user contents\n").unwrap();

    let error = write_repository_file_atomic_with(
        root.path(),
        relative,
        AtomicWriteOptions {
            expected_leaf: RepositoryFileLeaf::RegularFile,
            desired_permissions: None,
            allow_symlink_replacement: false,
            create_parents: true,
            temporary_directory: None,
        },
        || Ok(()),
        |temporary: &mut File| {
            temporary.write_all(b"partial Jig contents\n")?;
            bail!("injected managed copy failure")
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("injected managed copy failure"), "{error}");
    assert_eq!(
        fs::read(root.path().join(relative)).unwrap(),
        b"user contents\n"
    );
    assert!(fs::read_dir(root.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".jig-write-")
    }));
}

#[cfg(unix)]
#[test]
fn atomic_rendered_copy_applies_rendered_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempdir().unwrap();
    let source_root = tempdir().unwrap();
    let relative = Path::new("scripts/jig");
    fs::create_dir(root.path().join("scripts")).unwrap();
    fs::write(root.path().join(relative), b"old\n").unwrap();
    fs::write(source_root.path().join("jig"), b"new\n").unwrap();
    fs::set_permissions(
        source_root.path().join("jig"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let permissions = fs::metadata(source_root.path().join("jig"))
        .unwrap()
        .permissions();

    copy_repository_regular_file_atomic_with_permissions(
        root.path(),
        relative,
        &source_root.path().join("jig"),
        permissions,
        RepositoryFileLeaf::RegularFile,
    )
    .unwrap();

    assert_eq!(fs::read(root.path().join(relative)).unwrap(), b"new\n");
    assert_eq!(
        fs::metadata(root.path().join(relative))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[cfg(unix)]
#[test]
fn repository_relative_file_leaf_validation_accepts_leaf_symlinks() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    symlink("target", root.path().join("linked")).unwrap();

    assert_eq!(
        validate_repository_relative_file_leaf(root.path(), Path::new("linked")).unwrap(),
        RepositoryFileLeaf::Symlink
    );
}

#[cfg(unix)]
#[test]
fn repository_relative_ancestor_validation_rejects_a_symlink_root() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().unwrap();
    fs::create_dir(parent.path().join("real-root")).unwrap();
    let root = parent.path().join("root");
    symlink("real-root", &root).unwrap();

    let error = validate_repository_relative_ancestors(&root, Path::new("child"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("real directory"), "{error}");
}
