use super::*;

#[test]
fn reference_locations_follow_commonmark_line_endings() {
    for endings in [
        ["\r", "\r", "\r"],
        ["\r\n", "\r\n", "\r\n"],
        ["\r", "\r\n", "\n"],
    ] {
        let text = format!(
            "[first](a.md){}[second](b.md){}![third](c.png){}[owner][undefined]\r",
            endings[0], endings[1], endings[2]
        );
        let refs = markdown_references(&text);
        assert_eq!(
            refs.iter()
                .map(|r| (r.line, r.target.as_str()))
                .collect::<Vec<_>>(),
            [(1, "a.md"), (2, "b.md"), (3, "c.png"), (4, "undefined")]
        );
        assert!(refs[3].problem.is_some());

        let root = tempdir().unwrap();
        let ctx = fixture(root.path(), "rust", None);
        fs::write(root.path().join("AGENTS.md"), text).unwrap();
        let report = check(&ctx).unwrap();
        assert_eq!(report["ok"], false);
        assert_eq!(
            report["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["line"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
    }
}

#[cfg(unix)]
#[test]
fn a_disappeared_discovery_directory_is_optional() {
    let root = tempdir().unwrap();
    let files = GuideFiles::new(root.path()).unwrap();
    // Keep the capability open while removing its directory. This exercises a
    // real NotFound boundary without racing another thread or mocking I/O.
    fs::remove_dir(root.path()).unwrap();
    let discovery = files.discover();
    assert!(discovery.errors.is_empty(), "{:?}", discovery.errors);
    assert!(discovery.guides.is_empty());
}
