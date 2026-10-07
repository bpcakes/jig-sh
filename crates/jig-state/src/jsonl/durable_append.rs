use super::*;

pub fn append_jsonl_locked<T: Serialize>(
    _guard: &JsonlWriteGuard,
    path: &Path,
    value: &T,
) -> Result<u64> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    Ok(file.metadata()?.len())
}
