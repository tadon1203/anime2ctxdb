use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Reads one record per line. An invalid line is an error with its path and line number.
pub fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let record = serde_json::from_str(line)
            .with_context(|| format!("invalid line {} in {}", index + 1, path.display()))?;
        records.push(record);
    }
    Ok(records)
}

/// Appends one line for each record. Each line is one write call. The data is synced to disk.
pub fn append_jsonl<T: Serialize>(path: &Path, records: &[T]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    for record in records {
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        file.write_all(line.as_bytes())
            .with_context(|| format!("cannot write {}", path.display()))?;
    }
    file.sync_all()
        .with_context(|| format!("cannot sync {}", path.display()))?;
    Ok(())
}

/// Writes a file through a temporary file in the same directory, then renames it.
pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let name = path
        .file_name()
        .with_context(|| format!("no file name in {}", path.display()))?;
    let temporary = path.with_file_name(format!("{}.tmp", name.to_string_lossy()));
    let mut file = File::create(&temporary)
        .with_context(|| format!("cannot create {}", temporary.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync {}", temporary.display()))?;
    drop(file);
    fs::rename(&temporary, path).with_context(|| format!("cannot rename to {}", path.display()))?;
    Ok(())
}

/// Reads one JSON file. An invalid file is an error with its path.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid JSON in {}", path.display()))
}

/// The text of a value that serializes as a snake_case string.
pub fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value).expect("an enum serializes to JSON") {
        serde_json::Value::String(text) => text,
        other => panic!("expected a string, got {other}"),
    }
}

/// Reads the value that `enum_text` wrote. A value that the app did not write is an error.
pub fn enum_from_text<T: DeserializeOwned>(text: &str) -> Result<T> {
    Ok(serde_json::from_value(serde_json::Value::String(
        text.to_string(),
    ))?)
}
