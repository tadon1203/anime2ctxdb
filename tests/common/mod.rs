use anime2ctxdb::{Action, Series, Status, SuggestionFilter, SuggestionId};
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

/// Approves every pending suggestion in the order that the list shows them.
pub fn approve_all_pending(series: &Series) {
    let pending: Vec<SuggestionId> = series
        .suggestions(&SuggestionFilter {
            status: Some(Status::Pending),
            kind: None,
        })
        .expect("the suggestions load")
        .into_iter()
        .map(|row| row.id)
        .collect();
    let decided = series
        .decide(&pending, Action::Approve)
        .expect("the decisions are made");
    assert_eq!(
        decided.skipped, 0,
        "every pending suggestion can be approved"
    );
}
