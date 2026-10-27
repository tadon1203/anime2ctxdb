use anime2ctxdb::{Language, Series, SourceKind};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn raw_subtitle() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/raw.ass")
}

/// Copies the sample subtitle to `dir/name`. The name carries the episode.
fn copy_subtitle(dir: &Path, name: &str) -> PathBuf {
    let target = dir.join(name);
    fs::copy(raw_subtitle(), &target).unwrap();
    target
}

/// Copies the sample subtitle with one more line, so the file hash differs.
fn copy_other_subtitle(dir: &Path, name: &str) -> PathBuf {
    let target = dir.join(name);
    let mut text = fs::read_to_string(raw_subtitle()).unwrap();
    text.push_str("\nDialogue: 0,0:59:00.00,0:59:01.00,Default,,0,0,0,,other\n");
    fs::write(&target, text).unwrap();
    target
}

fn write_video(dir: &Path, name: &str) -> PathBuf {
    let target = dir.join(name);
    fs::write(&target, b"not really a video").unwrap();
    target
}

fn new_series(root: &TempDir) -> Series {
    Series::create(&root.path().join("show")).unwrap()
}

#[test]
fn detect_reads_kind_episode_and_language() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let subtitle = copy_subtitle(inputs.path(), "Show - 01.ass");
    let video = write_video(inputs.path(), "Show - 02.mkv");

    let records = series.read_files(&[subtitle, video]).unwrap();

    assert_eq!(records[0].kind, SourceKind::Subtitle);
    assert_eq!(records[0].episode, Some(1));
    assert_eq!(records[0].language, Some(Language::Ja));
    assert_eq!(records[1].kind, SourceKind::Video);
    assert_eq!(records[1].episode, Some(2));
    assert_eq!(records[1].language, None);
}

#[test]
fn detect_reads_each_episode_pattern() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let cases = [
        ("[Group] Show 第5話.mkv", Some(5)),
        ("Show E07.mkv", Some(7)),
        ("Show #9.mp4", Some(9)),
        ("Show [4].webm", Some(4)),
        ("Show.avi", None),
    ];
    for (name, expected) in cases {
        let path = write_video(inputs.path(), name);
        let records = series.read_files(&[path]).unwrap();
        assert_eq!(records[0].episode, expected, "file name {name}");
    }
}

#[test]
fn detect_rejects_an_unknown_file_kind() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let text = write_video(inputs.path(), "notes.txt");

    assert!(series.read_files(&[text]).is_err());
}

#[test]
fn save_rejects_episode_zero() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let path = copy_subtitle(inputs.path(), "Show - 00.ass");
    let records = series.read_files(&[path]).unwrap();
    assert_eq!(records[0].episode, Some(0));

    let error = series.add_sources(&records).unwrap_err();

    assert!(format!("{error:#}").contains("1 or more"));
    assert!(series.sources().unwrap().is_empty());
}

#[test]
fn save_rejects_two_ja_subtitles_for_one_episode() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let first = copy_subtitle(inputs.path(), "a - 01.ass");
    let second = copy_other_subtitle(inputs.path(), "b - 01.ass");
    let records = series.read_files(&[first, second]).unwrap();

    let error = series.add_sources(&records).unwrap_err();

    assert!(format!("{error:#}").contains("more than one Ja subtitle"));
}

#[test]
fn save_rejects_a_gap_and_names_the_missing_episode() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let path = copy_subtitle(inputs.path(), "Show - 02.ass");
    let records = series.read_files(&[path]).unwrap();

    let error = series.add_sources(&records).unwrap_err();

    assert!(format!("{error:#}").contains("episode 1 is missing"));
}

#[test]
fn save_rejects_a_video_with_a_language() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let path = write_video(inputs.path(), "Show - 01.mkv");
    let mut records = series.read_files(&[path]).unwrap();
    records[0].language = Some(Language::Ja);

    let error = series.add_sources(&records).unwrap_err();

    assert!(format!("{error:#}").contains("a video has no language"));
}

#[test]
fn repeated_import_returns_the_current_record_with_the_new_path() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let first = copy_subtitle(inputs.path(), "Show - 01.ass");
    let mut records = series.read_files(&[first]).unwrap();
    records[0].episode = Some(1);
    series.add_sources(&records).unwrap();

    let copy_dir = TempDir::new().unwrap();
    let again = copy_subtitle(copy_dir.path(), "Other name.ass");
    let detected = series.read_files(std::slice::from_ref(&again)).unwrap();

    assert_eq!(detected[0].episode, Some(1));
    assert_eq!(detected[0].path, again);
    assert_eq!(series.sources().unwrap().len(), 1);
}

#[test]
fn save_keeps_the_original_files_unchanged() {
    let inputs = TempDir::new().unwrap();
    let series = new_series(&inputs);
    let subtitle = copy_subtitle(inputs.path(), "Show - 01.ass");
    let before = fs::read(&subtitle).unwrap();
    let records = series.read_files(std::slice::from_ref(&subtitle)).unwrap();

    series.add_sources(&records).unwrap();

    assert_eq!(fs::read(&subtitle).unwrap(), before);
}

#[test]
fn a_broken_sources_line_fails_with_path_and_line_number() {
    let inputs = TempDir::new().unwrap();
    let root = inputs.path().join("show");
    let series = Series::create(&root).unwrap();
    let path = copy_subtitle(inputs.path(), "Show - 01.ass");
    let records = series.read_files(&[path]).unwrap();
    series.add_sources(&records).unwrap();
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(root.join("sources.jsonl"))
        .unwrap();
    std::io::Write::write_all(&mut file, b"{broken\n").unwrap();

    let error = series.sources().unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("sources.jsonl"), "{message}");
    assert!(message.contains("line 2"), "{message}");
}

#[test]
fn index_is_built_again_after_it_is_deleted() {
    let inputs = TempDir::new().unwrap();
    let root = inputs.path().join("show");
    let series = Series::create(&root).unwrap();
    let path = copy_subtitle(inputs.path(), "Show - 01.ass");
    let records = series.read_files(&[path]).unwrap();
    series.add_sources(&records).unwrap();

    fs::remove_file(root.join("index.sqlite")).unwrap();
    let reopened = Series::open(&root).unwrap();

    assert_eq!(reopened.sources().unwrap().len(), 1);
    assert!(root.join("index.sqlite").exists());
}

#[test]
fn create_rejects_a_directory_that_is_not_empty() {
    let inputs = TempDir::new().unwrap();
    fs::write(inputs.path().join("note.txt"), b"x").unwrap();

    assert!(Series::create(inputs.path()).is_err());
}
