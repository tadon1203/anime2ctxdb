use anime2ctxdb::{
    EpisodeOrigin, Language, PromptKind, Removed, Series, Severity, SourceId, SourceKind,
};
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
    fs::write(&target, name.as_bytes()).expect("the video file is written");
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
fn save_allows_a_gap_and_check_warns_about_the_missing_episode() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let path = copy_subtitle(inputs.path(), "Show - 02.ass");
    let records = series.read_files(&[path]).expect("the file is read");

    let problems = Series::check_sources(&[], &records);
    series.add_sources(&records).expect("a gap is allowed");

    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].severity, Severity::Warning);
    assert_eq!(problems[0].row, None);
    assert!(problems[0].message.contains("no subtitle for episode 1"));
}

#[test]
fn check_marks_the_rows_with_a_problem() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let first = copy_subtitle(inputs.path(), "a - 01.ass");
    let second = copy_other_subtitle(inputs.path(), "b - 01.ass");
    let video = write_video(inputs.path(), "Show.mkv");
    let records = series
        .read_files(&[first, second, video])
        .expect("the files are read");

    let problems = Series::check_sources(&[], &records);

    let errors: Vec<Option<usize>> = problems
        .iter()
        .filter(|problem| problem.severity == Severity::Error)
        .map(|problem| problem.row)
        .collect();
    assert_eq!(errors, [Some(0), Some(1)]);
    assert!(problems.iter().any(|problem| problem.row == Some(2)
        && problem.severity == Severity::Warning
        && problem.message.contains("no episode")));
}

#[test]
fn check_warns_when_the_episode_already_has_a_saved_source() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let saved = write_video(inputs.path(), "a - 01.mkv");
    let records = series.read_files(&[saved]).expect("the file is read");
    series.add_sources(&records).expect("the sources are added");
    let other = inputs.path().join("other");
    fs::create_dir(&other).expect("the directory is made");
    let again = {
        let path = other.join("b - 01.mkv");
        fs::write(&path, b"another video").expect("the video file is written");
        path
    };
    let pending = series.read_files(&[again]).expect("the file is read");

    let problems = Series::check_sources(&series.sources().expect("the sources load"), &pending);

    assert!(problems.iter().any(|problem| problem.row == Some(0)
        && problem.severity == Severity::Warning
        && problem.message.contains("already has a saved source")));
}

#[test]
fn scan_reads_a_folder_with_its_subfolders() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let show = inputs.path().join("Folder");
    let season = show.join("Season 1");
    fs::create_dir_all(&season).expect("the directories are made");
    copy_subtitle(&show, "Show - 01.ass");
    write_video(&season, "Show - 02.mkv");
    write_video(&season, "cover.jpg");
    fs::write(
        show.join("Show - 01 copy.ass"),
        fs::read(raw_subtitle()).expect("reads"),
    )
    .expect("the copy is written");
    fs::write(season.join("broken - 03.ass"), "not a subtitle").expect("the file is written");

    let mut steps = Vec::new();
    let scan = series
        .scan(&[show], |done, total| steps.push((done, total)))
        .expect("the folder is scanned");

    assert_eq!(scan.detected.len(), 2);
    assert_eq!(scan.skipped, 2);
    assert_eq!(scan.failed.len(), 1);
    assert!(scan.failed[0].path.ends_with("broken - 03.ass"));
    assert_eq!(steps.last(), Some(&(4, 4)));
}

#[test]
fn scan_proposes_an_episode_from_the_folder_name_and_a_sibling() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let folder = inputs.path().join("Show 第3話");
    fs::create_dir(&folder).expect("the directory is made");
    write_video(&folder, "subs.mkv");
    let by_folder = inputs.path().join("Season 2");
    fs::create_dir(&by_folder).expect("the directory is made");
    write_video(&by_folder, "movie.mkv");
    let pair = inputs.path().join("pair");
    fs::create_dir(&pair).expect("the directory is made");
    write_video(&pair, "clip [5].mkv");
    copy_subtitle(&pair, "clip [5].ja.ass");
    copy_other_subtitle(&pair, "01.ass");

    let scan = series
        .scan(&[inputs.path().to_path_buf()], |_, _| {})
        .expect("the folder is scanned");

    let find = |name: &str| {
        scan.detected
            .iter()
            .find(|item| item.source.path.ends_with(name))
            .unwrap_or_else(|| panic!("{name} is detected"))
    };
    assert_eq!(find("subs.mkv").source.episode, Some(3));
    assert_eq!(find("subs.mkv").origin, EpisodeOrigin::FolderName);
    assert_eq!(find("movie.mkv").source.episode, None);
    assert_eq!(find("movie.mkv").origin, EpisodeOrigin::Unknown);
    assert_eq!(find("clip [5].ja.ass").source.episode, Some(5));
    assert_eq!(find("01.ass").source.episode, Some(1));
    assert_eq!(find("01.ass").origin, EpisodeOrigin::FileName);
}

#[test]
fn new_dir_rejects_a_bad_name_and_a_directory_that_is_not_empty() {
    let parent = TempDir::new().expect("a temporary directory");
    fs::create_dir(parent.path().join("used")).expect("the directory is made");
    fs::write(parent.path().join("used").join("note.txt"), b"x").expect("the note is written");

    for name in ["", "  ", "a/b", "a:b", "..", "end.", "used"] {
        assert!(
            Series::new_dir(parent.path(), name).is_err(),
            "name {name:?}"
        );
    }
    let dir = Series::new_dir(parent.path(), " Show 2 ").expect("the name is fine");
    assert_eq!(dir, parent.path().join("Show 2"));
}

#[test]
fn list_finds_only_the_series_directories() {
    let root = TempDir::new().expect("a temporary directory");
    assert_eq!(
        Series::list(&root.path().join("missing")).expect("a missing root is fine"),
        Vec::<PathBuf>::new()
    );
    for name in ["B show", "A show"] {
        Series::create(&root.path().join(name)).expect("the series is made");
    }
    fs::create_dir(root.path().join("other")).expect("the directory is made");
    fs::write(root.path().join("note.txt"), b"x").expect("the note is written");

    let dirs = Series::list(root.path()).expect("the root is read");
    assert_eq!(
        dirs,
        vec![root.path().join("A show"), root.path().join("B show")]
    );
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
fn remove_deletes_the_record_and_the_copy_but_not_the_original() {
    let inputs = TempDir::new().expect("a temporary directory");
    let root = inputs.path().join("show");
    let series = Series::create(&root).expect("the series is created");
    let subtitle = copy_subtitle(inputs.path(), "Show - 01.ass");
    let video = write_video(inputs.path(), "Show - 01.mkv");
    let records = series
        .read_files(&[subtitle.clone(), video.clone()])
        .expect("the files are read");
    series.add_sources(&records).expect("the sources are added");
    let copy = root.join("sources").join(format!("{}.ass", records[0].id));
    assert!(copy.exists());

    let ids = [records[0].id.clone(), records[1].id.clone()];
    let removed = series
        .remove_sources(&ids)
        .expect("the sources are removed");

    assert_eq!(
        removed,
        Removed {
            removed: 2,
            blocked: 0
        }
    );
    assert_eq!(series.sources().expect("the sources load").len(), 0);
    assert!(!copy.exists());
    assert!(subtitle.exists());
    assert!(video.exists());
    assert!(
        series
            .progress()
            .expect("the progress loads")
            .iter()
            .all(|item| !item.has_subtitle)
    );
}

#[test]
fn remove_keeps_a_source_that_an_answer_uses() {
    let root = TempDir::new().expect("a temporary directory");
    let series = new_series(&root);
    let first = copy_subtitle(root.path(), "Show - 01.ass");
    let second = copy_other_subtitle(root.path(), "Show - 02.ass");
    let records = series
        .read_files(&[first, second])
        .expect("the files are read");
    series.add_sources(&records).expect("the sources are added");
    let saved = series.sources().expect("the sources load");
    series
        .submit_answer(
            PromptKind::Speakers,
            &[1],
            r#"{ "new_characters": [{ "ref": "N1", "name": "x", "evidence": ["L1"] }] }"#,
        )
        .expect("the answer is submitted");
    let ids: Vec<SourceId> = saved.iter().map(|record| record.id.clone()).collect();

    let blocked = series.blocked_removals(&ids).expect("the check runs");
    let removed = series
        .remove_sources(&ids)
        .expect("the sources are removed");

    assert_eq!(blocked, [ids[0].clone()]);
    assert_eq!(
        removed,
        Removed {
            removed: 1,
            blocked: 1
        }
    );
    let left = series.sources().expect("the sources load");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, ids[0]);
    assert!(
        root.path()
            .join("show")
            .join("sources")
            .join(format!("{}.ass", ids[0]))
            .exists()
    );
}

#[test]
fn a_removed_file_can_be_imported_again() {
    let inputs = TempDir::new().expect("a temporary directory");
    let series = new_series(&inputs);
    let subtitle = copy_subtitle(inputs.path(), "Show - 01.ass");
    let records = series
        .read_files(std::slice::from_ref(&subtitle))
        .expect("the file is read");
    series.add_sources(&records).expect("the sources are added");
    series
        .remove_sources(&[records[0].id.clone()])
        .expect("the source is removed");

    series
        .add_sources(&records)
        .expect("the file is imported again");

    assert_eq!(series.sources().expect("the sources load").len(), 1);
}

#[test]
fn create_rejects_a_directory_that_is_not_empty() {
    let inputs = TempDir::new().expect("a temporary directory");
    fs::write(inputs.path().join("note.txt"), b"x").expect("the note is written");

    assert!(Series::create(inputs.path()).is_err());
}
