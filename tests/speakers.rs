use anime2ctxdb::{
    Action, PromptKind, Series, Status, SuggestionFilter, SuggestionId, SuggestionRow,
};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const ANSWER: &str = include_str!("fixtures/answer_new_character.json");
const UNKNOWN_REF: &str = include_str!("fixtures/answer_unknown_ref.json");

fn raw_subtitle() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/raw.ass")
}

/// Creates a series with the sample subtitle as episode 1.
fn series_with_episode_one(root: &Path) -> Series {
    let series = Series::create(&root.join("show")).expect("the series is created");
    let path = root.join("Show - 01.ass");
    fs::copy(raw_subtitle(), &path).expect("the sample subtitle copies");
    let records = series.read_files(&[path]).expect("the file is read");
    series.add_sources(&records).expect("the sources are added");
    series
}

fn all_rows(series: &Series) -> Vec<SuggestionRow> {
    series
        .suggestions(&SuggestionFilter {
            status: None,
            kind: None,
        })
        .expect("the suggestions load")
}

#[test]
fn create_prompt_has_the_transcript_and_the_labels() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());

    let prompt = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");

    assert_eq!(prompt.id.0, 1);
    assert!(prompt.text.contains("L1 "), "transcript line missing");
    assert!(
        prompt.text.contains("（明智あんな）"),
        "speaker label missing"
    );
    assert!(!prompt.refs.contains_key("C1"), "no characters yet");
}

#[test]
fn create_prompt_fails_for_an_episode_without_a_ja_subtitle() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());

    let error = series
        .create_prompt(PromptKind::Speakers, 2)
        .expect_err("episode 2 has no subtitle");

    assert!(format!("{error:#}").contains("no Ja subtitle"));
}

#[test]
fn submit_answer_creates_suggestions_and_rejects_a_second_answer() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());
    let prompt = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");

    let count = series
        .submit_answer(prompt.id, ANSWER)
        .expect("the answer is submitted");
    let again = series
        .submit_answer(prompt.id, ANSWER)
        .expect_err("the second answer is rejected");

    assert_eq!(count, 6);
    assert!(format!("{again:#}").contains("prompt 1 already has an answer"));
}

#[test]
fn invalid_answer_writes_no_file() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());
    let prompt = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");

    let error = series
        .submit_answer(prompt.id, UNKNOWN_REF)
        .expect_err("the unknown reference is rejected");

    assert!(format!("{error:#}").contains("unknown reference C9"));
    assert!(!root.path().join("show/answers/1.json").exists());
}

#[test]
fn approve_all_shown_makes_speakers_known() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());
    let prompt = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");
    series
        .submit_answer(prompt.id, ANSWER)
        .expect("the answer is submitted");
    let before = series.progress().expect("the progress loads")[0]
        .unknown_lines
        .expect("episode 1 has a Ja subtitle");
    let pending = series
        .suggestions(&SuggestionFilter {
            status: Some(Status::Pending),
            kind: None,
        })
        .expect("the suggestions load");
    let ids: Vec<SuggestionId> = pending.iter().map(|row| row.id.clone()).collect();

    let decided = series
        .decide(&ids, Action::Approve)
        .expect("the decisions are made");

    assert_eq!((decided.applied, decided.skipped), (6, 0));
    let after = series.progress().expect("the progress loads")[0]
        .unknown_lines
        .expect("episode 1 has a Ja subtitle");
    assert!(
        after < before,
        "unknown lines went from {before} to {after}"
    );
}

#[test]
fn a_rejected_character_makes_its_label_invalid() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());
    let prompt = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");
    series
        .submit_answer(prompt.id, ANSWER)
        .expect("the answer is submitted");

    series
        .decide(&[SuggestionId("1-1".to_string())], Action::Reject)
        .expect("the decision is made");

    let label = all_rows(&series)
        .into_iter()
        .find(|row| row.id.0 == "1-2")
        .expect("suggestion 1-2 exists");
    assert_eq!(label.status, Status::Invalid);
}

#[test]
fn a_label_waits_while_its_character_is_pending() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());
    let prompt = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");
    series
        .submit_answer(prompt.id, ANSWER)
        .expect("the answer is submitted");

    let decided = series
        .decide(&[SuggestionId("1-2".to_string())], Action::Approve)
        .expect("the decision is made");

    assert_eq!((decided.applied, decided.skipped), (0, 1));
    let label = all_rows(&series)
        .into_iter()
        .find(|row| row.id.0 == "1-2")
        .expect("suggestion 1-2 exists");
    assert!(label.blocked);
    assert_eq!(label.status, Status::Pending);
}

#[test]
fn revoke_returns_an_approved_character_to_pending() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());
    let prompt = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");
    series
        .submit_answer(prompt.id, ANSWER)
        .expect("the answer is submitted");
    series
        .decide(&[SuggestionId("1-1".to_string())], Action::Approve)
        .expect("the decision is made");

    series
        .decide(&[SuggestionId("1-1".to_string())], Action::Revoke)
        .expect("the decision is made");

    let character = all_rows(&series)
        .into_iter()
        .find(|row| row.id.0 == "1-1")
        .expect("suggestion 1-1 exists");
    assert_eq!(character.status, Status::Pending);
}

#[test]
fn revoking_a_line_speaker_brings_back_its_unknown_line() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());
    let prompt = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");
    series
        .submit_answer(prompt.id, ANSWER)
        .expect("the answer is submitted");
    let ids: Vec<SuggestionId> = all_rows(&series).into_iter().map(|row| row.id).collect();
    series
        .decide(&ids, Action::Approve)
        .expect("the decisions are made");
    let approved = series.progress().expect("the progress loads")[0]
        .unknown_lines
        .expect("episode 1 has a Ja subtitle");

    series
        .decide(&[SuggestionId("1-5".to_string())], Action::Revoke)
        .expect("the decision is made");

    let revoked = series.progress().expect("the progress loads")[0]
        .unknown_lines
        .expect("episode 1 has a Ja subtitle");
    assert!(
        revoked > approved,
        "unknown lines went from {approved} to {revoked}"
    );
}

#[test]
fn a_second_approval_of_the_same_label_is_skipped() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episode_one(root.path());
    let first = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");
    series
        .submit_answer(first.id, ANSWER)
        .expect("the answer is submitted");
    series
        .decide(
            &[
                SuggestionId("1-1".to_string()),
                SuggestionId("1-2".to_string()),
            ],
            Action::Approve,
        )
        .expect("the decisions are made");
    let second = series
        .create_prompt(PromptKind::Speakers, 1)
        .expect("the prompt is created");
    series
        .submit_answer(second.id, ANSWER)
        .expect("the answer is submitted");

    let decided = series
        .decide(&[SuggestionId("2-2".to_string())], Action::Approve)
        .expect("the decision is made");

    assert_eq!((decided.applied, decided.skipped), (0, 1));
}
