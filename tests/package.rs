mod common;

use anime2ctxdb::{Importance, Position, PromptKind, Series};
use common::series_with_episodes;
use tempfile::TempDir;

/// The answer for episode 1. Each `{name}` is the short reference of a line that starts at a time.
///
/// Times, from the sample: the early fact ends at about 13 s, the scene that is in progress at
/// 50 s starts at 25 s and runs to the end of the episode, and the late fact and the late name end
/// at about 87 s.
const STORY_ANSWER: &str = r#"{
  "new_entities": [
    { "kind": "place", "name": "教室", "importance": "main", "evidence": ["{a}"] },
    { "kind": "character", "name": "未来の名前", "importance": "main", "evidence": ["{g}"] }
  ],
  "facts": [
    { "entities": ["教室"], "text": "早い事実", "evidence": ["{c}", "{b}"] },
    { "entities": ["未来の名前"], "text": "遅い事実", "evidence": ["{c}", "{g}"] }
  ],
  "scenes": [
    { "from": "{a}", "place": "教室", "summary": "終わった場面" },
    { "from": "{e}", "place": "教室", "summary": "進行中の場面" }
  ]
}"#;

/// The short reference of the first line of the transcript that starts at or after `clock`.
/// A speakers transcript line has the form `L1 00:00 text`.
fn line_from(transcript: &str, clock: &str) -> String {
    transcript
        .lines()
        .find_map(|line| {
            let mut parts = line.split(' ');
            let reference = parts.next()?;
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
        ("e", "00:25"),
        ("g", "01:25"),
    ]
    .iter()
    .fold(STORY_ANSWER.to_string(), |answer, (name, clock)| {
        answer.replace(&format!("{{{name}}}"), &line_from(transcript, clock))
    })
}

/// The line references have the same numbers in every prompt of an episode. Only the speakers
/// prompt has the times.
fn times_of(series: &Series) -> String {
    series
        .prompt(PromptKind::Speakers, 1)
        .expect("the prompt is made")
        .text
}

/// Makes episode 1 with the story answer.
fn story_series(root: &std::path::Path) -> Series {
    let series = series_with_episodes(root, 1);
    let times = times_of(&series);
    series
        .submit_answer(PromptKind::Story, 1, &story_answer(&times))
        .expect("the answer is submitted");
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

    let text = package_at(&series, 1, 1_500_000);

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

/// An answer with a minor entity and a fact about it, known at about 13 s.
fn minor_series(root: &std::path::Path) -> Series {
    let series = series_with_episodes(root, 1);
    let times = times_of(&series);
    let b = line_from(&times, "00:10");
    let answer = format!(
        r#"{{
          "new_entities": [{{ "kind": "object", "name": "ティアラ", "evidence": ["{b}"] }}],
          "facts": [{{ "entities": ["ティアラ"], "text": "ティアラの事実", "evidence": ["{b}"] }}]
        }}"#
    );
    series
        .submit_answer(PromptKind::Story, 1, &answer)
        .expect("the answer is submitted");
    series
}

#[test]
fn a_minor_entity_and_its_facts_are_left_out_of_the_package() {
    let root = TempDir::new().expect("a temporary directory");
    let series = minor_series(root.path());

    let text = package_at(&series, 1, 120_000);

    assert!(!text.contains("ティアラ"));
}

#[test]
fn a_hand_setting_puts_a_minor_entity_in_the_package() {
    let root = TempDir::new().expect("a temporary directory");
    let series = minor_series(root.path());

    series
        .set_importance("ティアラ", Some(Importance::Recurring))
        .expect("the importance is set");

    let text = package_at(&series, 1, 120_000);
    assert!(text.contains("name: \"ティアラ\""));
    assert!(text.contains("ティアラの事実"));
}

#[test]
fn a_character_has_sample_lines_in_the_package() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    let answer = r#"{
      "new_characters": [{ "name": "明智あんな", "importance": "main", "evidence": ["L1"] }],
      "labels": [{ "label": "明智あんな", "kind": "character", "character": "明智あんな" }]
    }"#;
    series
        .submit_answer(PromptKind::Speakers, 1, answer)
        .expect("the answer is submitted");

    let text = package_at(&series, 2, 0);

    let entity = text
        .split_once("name: \"明智あんな\"")
        .expect("the character is listed")
        .1;
    assert!(
        entity.trim_start().starts_with("kind: \"character\""),
        "the entity has no reading"
    );
    assert!(entity.contains("lines:"), "the sample lines are missing");
}
