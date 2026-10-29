use anime2ctxdb::Series;
use std::fs;
use std::path::{Path, PathBuf};

pub fn raw_subtitle() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/raw.ass")
}

/// Creates a series with the sample subtitle as episodes 1 to `count`.
///
/// Each copy has its own trailing newlines, so each episode has its own source ID.
pub fn series_with_episodes(root: &Path, count: u32) -> Series {
    let series = Series::create(&root.join("show")).expect("the series is created");
    for number in 1..=count {
        let path = root.join(format!("Show - {number:02}.ass"));
        let mut bytes = fs::read(raw_subtitle()).expect("the sample subtitle is read");
        bytes.extend(std::iter::repeat_n(b'\n', number as usize));
        fs::write(&path, bytes).expect("the episode is written");
        let records = series.read_files(&[path]).expect("the file is read");
        series.add_sources(&records).expect("the sources are added");
    }
    series
}
