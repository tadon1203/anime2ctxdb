mod common;

use anime2ctxdb::{Position, PromptKind, Series};
use common::{approve_all_pending, series_with_episodes};
use tempfile::TempDir;

/// The answer for episode 1. Each `{name}` is the short reference of a line that starts at a time.
///
/// Times, from the sample: the early fact ends at about 13 s, the scene that is in progress at
/// 50 s runs from 25 s to 70 s, and the late fact and the late name end at about 87 s.
const STORY_ANSWER: &str = r#"{
  "new_entities": [
    { "ref": "N1", "kind": "place", "name": "教室", "evidence": ["{a}"] },
    { "ref": "N2", "kind": "character", "name": "未来の名前", "evidence": ["{g}"] }
  ],
  "facts": [
    { "entities": ["N1"], "text": "早い事実", "evidence": ["{c}", "{b}"] },
    { "entities": ["N2"], "text": "遅い事実", "evidence": ["{c}", "{g}"] }
  ],
  "scenes": [
    { "start": "{a}", "end": "{d}", "place": "N1", "summary": "終わった場面" },
    { "start": "{e}", "end": "{f}", "place": "N1", "summary": "進行中の場面" }
  ]
}"#;

/// The short reference of the first line of the transcript that starts at or after `clock`.
/// A transcript line has the form `L1 00:00 text`.
fn line_from(transcript: &str, clock: &str) -> String {
    transcript
        .lines()
        .find_map(|line| {
            let mut parts = line.split(' ');
            let reference = parts.next()?;
            let _episode = parts.next()?;
            let time = parts.next()?;
            (reference.starts_with('L') && time >= clock).then(|| reference.to_string())
        })
        .expect("the transcript has a line from this time")
}

/// Fills the story answer with the lines of the prompt.
fn story_answer(transcript: &str) -> String {
    [
        ("a", "00:00"),
        ("b", "00:10"),
        ("c", "00:02"),
        ("d", "00:20"),
        ("e", "00:25"),
        ("f", "01:05"),
        ("g", "01:25"),
    ]
    .iter()
    .fold(STORY_ANSWER.to_string(), |answer, (name, clock)| {
        answer.replace(&format!("{{{name}}}"), &line_from(transcript, clock))
    })
}

/// Makes episode 1 with the story answer, and approves it.
fn story_series(root: &std::path::Path) -> Series {
    let series = series_with_episodes(root, 1);
    let prompt = series
        .prompt(PromptKind::Story, &[1])
        .expect("the prompt is made");
    series
        .submit_answer(PromptKind::Story, &[1], &story_answer(&prompt.text))
        .expect("the answer is submitted");
    approve_all_pending(&series);
    series
}

fn package_at(series: &Series, episode: u32, time_ms: u32) -> String {
    series
        .build_package(Position { episode, time_ms })
        .expect("the package is built")
}

#[test]
fn the_package_has_nothing_from_after_its_position() {
    let root = TempDir::new().expect("a temporary directory");
    let series = story_series(root.path());

    let text = package_at(&series, 1, 50_000);

    assert!(text.contains("position: { episode: 1, time: \"00:50\" }"));
    assert!(
        text.contains("早い事実"),
        "a fact known before 00:50 is missing"
    );
    assert!(text.contains("終わった場面"), "an ended scene is missing");
    assert!(!text.contains("遅い事実"), "a fact from after 00:50 leaked");
    assert!(
        !text.contains("未来の名前"),
        "a name from after 00:50 leaked"
    );
    assert!(
        !text.contains("進行中の場面"),
        "the summary of a scene in progress leaked"
    );
}

#[test]
fn the_current_scene_has_its_place_and_its_lines() {
    let root = TempDir::new().expect("a temporary directory");
    let series = story_series(root.path());

    let text = package_at(&series, 1, 50_000);

    let current = text
        .split_once("current_scene:")
        .expect("the package has a current scene")
        .1;
    assert!(current.contains("place: \"教室\""));
    assert!(current.contains("lines:"));
}

#[test]
fn a_scene_and_a_fact_appear_after_their_end_or_since() {
    let root = TempDir::new().expect("a temporary directory");
    let series = story_series(root.path());

    let text = package_at(&series, 1, 120_000);

    assert!(
        text.contains("進行中の場面"),
        "the scene is missing after it ends"
    );
    assert!(text.contains("遅い事実"), "the later fact is missing");
    assert!(text.contains("未来の名前"), "the later entity is missing");
}

#[test]
fn the_package_is_the_same_after_the_index_is_built_again() {
    let root = TempDir::new().expect("a temporary directory");
    let series = story_series(root.path());
    let before = package_at(&series, 1, 50_000);

    std::fs::remove_file(series.dir().join("index.sqlite")).expect("the index is removed");
    let after = package_at(&series, 1, 50_000);

    assert_eq!(before, after);
}

#[test]
fn an_episode_without_a_ja_subtitle_is_an_error() {
    let root = TempDir::new().expect("a temporary directory");
    let series = story_series(root.path());

    let error = series
        .build_package(Position {
            episode: 9,
            time_ms: 0,
        })
        .expect_err("episode 9 has no subtitle");

    assert!(format!("{error:#}").contains("no Ja subtitle"));
}
