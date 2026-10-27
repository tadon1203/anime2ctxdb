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
    fs::copy(raw_subtitle(), &target).expect("the sample subtitle copies");
    target
}

/// Copies the sample subtitle with one more line, so the file hash differs.
fn copy_other_subtitle(dir: &Path, name: &str) -> PathBuf {
    let target = dir.join(name);
    let mut text = fs::read_to_string(raw_subtitle()).expect("the sample subtitle reads");
    text.push_str("\nDialogue: 0,0:59:00.00,0:59:01.00,Default,,0,0,0,,other\n");
    fs::write(&target, text).expect("the copy is written");
    target
}

fn write_video(dir: &Path, name: &str) -> PathBuf {
    let target = dir.join(name);
    fs::write(&target, b"not really a video").expect("the video file is written");
    target
}

fn new_series(root: &TempDir) -> Series {
    Series::create(&root.path().join("show")).expect("the series is created")
}

#[test]
fn detect_reads_kind_episode_and_language() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let subtitle = copy_subtitle(inputs.path(), "Show - 01.ass");
    let video = write_video(inputs.path(), "Show - 02.mkv");

    let records = series
        .read_files(&[subtitle, video])
        .expect("the files are read");

    assert_eq!(records[0].kind, SourceKind::Subtitle);
    assert_eq!(records[0].episode, Some(1));
    assert_eq!(records[0].language, Some(Language::Ja));
    assert_eq!(records[1].kind, SourceKind::Video);
    assert_eq!(records[1].episode, Some(2));
    assert_eq!(records[1].language, None);
}

#[test]
fn detect_reads_each_episode_pattern() {
    let inputs = TempDir::new().expect("a temporary directory");
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
        let records = series.read_files(&[path]).expect("the file is read");
        assert_eq!(records[0].episode, expected, "file name {name}");
    }
}

#[test]
fn detect_rejects_an_unknown_file_kind() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let text = write_video(inputs.path(), "notes.txt");

    assert!(series.read_files(&[text]).is_err());
}

#[test]
fn save_rejects_episode_zero() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let path = copy_subtitle(inputs.path(), "Show - 00.ass");
    let records = series.read_files(&[path]).expect("the file is read");
    assert_eq!(records[0].episode, Some(0));

    let error = series
        .add_sources(&records)
        .expect_err("episode 0 is rejected");

    assert!(format!("{error:#}").contains("1 or more"));
    assert_eq!(series.sources().expect("the sources load").len(), 0);
}

#[test]
fn save_rejects_two_ja_subtitles_for_one_episode() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let first = copy_subtitle(inputs.path(), "a - 01.ass");
    let second = copy_other_subtitle(inputs.path(), "b - 01.ass");
    let records = series
        .read_files(&[first, second])
        .expect("the files are read");

    let error = series
        .add_sources(&records)
        .expect_err("two Ja subtitles are rejected");

    assert!(format!("{error:#}").contains("more than one Ja subtitle"));
}

#[test]
fn save_rejects_a_gap_and_names_the_missing_episode() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let path = copy_subtitle(inputs.path(), "Show - 02.ass");
    let records = series.read_files(&[path]).expect("the file is read");

    let error = series.add_sources(&records).expect_err("a gap is rejected");

    assert!(format!("{error:#}").contains("episode 1 is missing"));
}

#[test]
fn save_rejects_a_video_with_a_language() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let path = write_video(inputs.path(), "Show - 01.mkv");
    let mut records = series.read_files(&[path]).expect("the file is read");
    records[0].language = Some(Language::Ja);

    let error = series
        .add_sources(&records)
        .expect_err("a video with a language is rejected");

    assert!(format!("{error:#}").contains("a video has no language"));
}

#[test]
fn repeated_import_returns_the_current_record_with_the_new_path() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let first = copy_subtitle(inputs.path(), "Show - 01.ass");
    let mut records = series.read_files(&[first]).expect("the file is read");
    records[0].episode = Some(1);
    series.add_sources(&records).expect("the sources are added");

    let copy_dir = TempDir::new().expect("a temporary directory");
    let again = copy_subtitle(copy_dir.path(), "Other name.ass");
    let detected = series
        .read_files(std::slice::from_ref(&again))
        .expect("the file is read");

    assert_eq!(detected[0].episode, Some(1));
    assert_eq!(detected[0].path, again);
    assert_eq!(series.sources().expect("the sources load").len(), 1);
}

#[test]
fn save_keeps_the_original_files_unchanged() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let subtitle = copy_subtitle(inputs.path(), "Show - 01.ass");
    let before = fs::read(&subtitle).expect("the subtitle reads");
    let records = series
        .read_files(std::slice::from_ref(&subtitle))
        .expect("the file is read");

    series.add_sources(&records).expect("the sources are added");

    assert_eq!(fs::read(&subtitle).expect("the subtitle reads"), before);
}

#[test]
fn a_broken_sources_line_fails_with_path_and_line_number() {
    let inputs = TempDir::new().expect("a temporary directory");
    let root = inputs.path().join("show");
    let series = Series::create(&root).expect("the series is created");
    let path = copy_subtitle(inputs.path(), "Show - 01.ass");
    let records = series.read_files(&[path]).expect("the file is read");
    series.add_sources(&records).expect("the sources are added");
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(root.join("sources.jsonl"))
        .expect("sources.jsonl opens");
    std::io::Write::write_all(&mut file, b"{broken\n").expect("the broken line is written");

    let error = series.sources().expect_err("the broken line is rejected");

    let message = format!("{error:#}");
    assert!(message.contains("sources.jsonl"), "{message}");
    assert!(message.contains("line 2"), "{message}");
}

#[test]
fn index_is_built_again_after_it_is_deleted() {
    let inputs = TempDir::new().expect("a temporary directory");
    let root = inputs.path().join("show");
    let series = Series::create(&root).expect("the series is created");
    let path = copy_subtitle(inputs.path(), "Show - 01.ass");
    let records = series.read_files(&[path]).expect("the file is read");
    series.add_sources(&records).expect("the sources are added");

    fs::remove_file(root.join("index.sqlite")).expect("the index is removed");
    let reopened = Series::open(&root).expect("the series opens");

    assert_eq!(reopened.sources().expect("the sources load").len(), 1);
    assert!(root.join("index.sqlite").exists());
}

#[test]
fn create_rejects_a_directory_that_is_not_empty() {
    let inputs = TempDir::new().expect("a temporary directory");
    fs::write(inputs.path().join("note.txt"), b"x").expect("the note is written");

    assert!(Series::create(inputs.path()).is_err());
}
