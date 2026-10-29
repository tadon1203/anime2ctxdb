mod common;

use anime2ctxdb::{PromptKind, Series};
use common::series_with_episodes;
use tempfile::TempDir;

const ANSWER: &str = include_str!("fixtures/answer_new_character.json");
const UNKNOWN_REF: &str = include_str!("fixtures/answer_unknown_ref.json");

/// An answer that introduces one character and nothing else.
fn only_character(name: &str) -> String {
    format!(r#"{{ "new_characters": [{{ "name": "{name}", "evidence": ["L1"] }}] }}"#)
}

/// An answer that gives the label 明智あんな to one character.
fn label_to(name: &str) -> String {
    format!(
        r#"{{ "new_characters": [{{ "name": "{name}", "evidence": ["L1"] }}],
             "labels": [{{ "label": "明智あんな", "kind": "character", "character": "{name}" }}] }}"#
    )
}

fn unknown_lines(series: &Series, episode: u32) -> u32 {
    series
        .progress()
        .expect("the progress loads")
        .into_iter()
        .find(|item| item.episode == episode)
        .and_then(|item| item.unknown_lines)
        .expect("the episode has a Ja subtitle")
}

#[test]
fn prompt_has_the_transcript_and_the_labels() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    let prompt = series
        .prompt(PromptKind::Speakers, 1)
        .expect("the prompt is made");

    assert!(prompt.text.contains("L1 "), "transcript line missing");
    assert!(
        prompt.text.contains("（明智あんな）"),
        "speaker label missing"
    );
    assert!(prompt.known.is_empty(), "no characters yet");
}

#[test]
fn prompt_is_not_saved_and_is_the_same_each_time() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    let first = series
        .prompt(PromptKind::Speakers, 1)
        .expect("the prompt is made");
    let second = series
        .prompt(PromptKind::Speakers, 1)
        .expect("the prompt is made");

    assert_eq!(first, second);
    let saved = std::fs::read_dir(root.path().join("show/answers"))
        .expect("the answers folder reads")
        .count();
    assert_eq!(saved, 0);
}

#[test]
fn prompt_fails_for_an_episode_without_a_ja_subtitle() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    let error = series
        .prompt(PromptKind::Speakers, 2)
        .expect_err("episode 2 has no subtitle");

    assert!(format!("{error:#}").contains("no Ja subtitle"));
}

#[test]
fn an_answer_is_data_at_once() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);
    let before = unknown_lines(&series, 1);

    let count = series
        .submit_answer(PromptKind::Speakers, 1, ANSWER)
        .expect("the answer is submitted");

    assert_eq!(count, 6);
    assert!(root.path().join("show/answers/speakers-1.json").exists());
    let data = series.data().expect("the data loads");
    assert_eq!(data.entities.len(), 1);
    assert_eq!(data.entities[0].name, "明智あんな");
    assert!(
        unknown_lines(&series, 1) < before,
        "the lines have speakers now"
    );
}

#[test]
fn a_new_answer_replaces_the_old_one_whole() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);
    let before = unknown_lines(&series, 1);
    series
        .submit_answer(PromptKind::Speakers, 1, ANSWER)
        .expect("the answer is submitted");

    series
        .submit_answer(PromptKind::Speakers, 1, &only_character("坂本みくる"))
        .expect("the answer is replaced");

    let data = series.data().expect("the data loads");
    let names: Vec<&str> = data
        .entities
        .iter()
        .map(|entity| entity.name.as_str())
        .collect();
    assert_eq!(names, ["坂本みくる"]);
    assert_eq!(unknown_lines(&series, 1), before);
}

#[test]
fn the_prompt_for_an_answered_episode_is_made_as_if_it_had_no_answer() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);
    series
        .submit_answer(PromptKind::Speakers, 1, ANSWER)
        .expect("the answer is submitted");

    let speakers = series
        .prompt(PromptKind::Speakers, 1)
        .expect("the prompt is made");
    let story = series
        .prompt(PromptKind::Story, 1)
        .expect("the prompt is made");

    assert!(speakers.known.is_empty(), "its own answer is left out");
    assert!(story.known.contains("明智あんな"), "the other kind stays");
}

#[test]
fn an_answer_to_an_earlier_episode_is_known_in_the_next_prompt() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    series
        .submit_answer(PromptKind::Speakers, 1, ANSWER)
        .expect("the answer is submitted");

    let prompt = series
        .prompt(PromptKind::Speakers, 2)
        .expect("the prompt is made");

    assert!(prompt.known.contains("明智あんな"));
    assert!(prompt.text.contains("明智あんな (あけちあんな)"));
}

#[test]
fn a_prompt_does_not_list_an_entity_from_a_later_episode() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    series
        .submit_answer(PromptKind::Speakers, 2, &only_character("坂本みくる"))
        .expect("the answer is submitted");

    let prompt = series
        .prompt(PromptKind::Story, 1)
        .expect("the prompt is made");

    assert!(prompt.known.is_empty());
}

#[test]
fn answers_with_one_name_give_one_entity() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    series
        .submit_answer(PromptKind::Speakers, 1, &only_character("坂本みくる"))
        .expect("the answer is submitted");
    series
        .submit_answer(PromptKind::Speakers, 2, &only_character("坂本みくる"))
        .expect("the answer is submitted");

    let data = series.data().expect("the data loads");

    assert_eq!(data.entities.len(), 1);
    assert_eq!(data.entities[0].since.episode, 1);
}

#[test]
fn the_later_episode_wins_when_two_answers_give_a_label_a_meaning() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    series
        .submit_answer(PromptKind::Speakers, 1, &label_to("坂本みくる"))
        .expect("the answer is submitted");
    series
        .submit_answer(PromptKind::Speakers, 2, &label_to("明智あんな"))
        .expect("the answer is submitted");

    let prompt = series
        .prompt(PromptKind::Story, 1)
        .expect("the prompt is made");

    assert!(prompt.text.contains("[明智あんな]"));
    assert!(!prompt.text.contains("[坂本みくる]"));
}

#[test]
fn an_unknown_character_writes_no_file() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    let error = series
        .submit_answer(PromptKind::Speakers, 1, UNKNOWN_REF)
        .expect_err("the unknown character is rejected");

    assert!(format!("{error:#}").contains("unknown entity 誰か"));
    assert!(!root.path().join("show/answers/speakers-1.json").exists());
}

#[test]
fn an_answer_marks_its_episode_as_answered_for_its_kind_only() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);
    assert!(
        series
            .answered_episodes(PromptKind::Speakers)
            .expect("the answers load")
            .is_empty()
    );

    series
        .submit_answer(PromptKind::Speakers, 1, ANSWER)
        .expect("the answer is submitted");

    let speakers = series
        .answered_episodes(PromptKind::Speakers)
        .expect("the answers load");
    let story = series
        .answered_episodes(PromptKind::Story)
        .expect("the answers load");
    assert_eq!(speakers.into_iter().collect::<Vec<_>>(), [1]);
    assert!(story.is_empty());
}
