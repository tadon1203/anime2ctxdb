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
    let series = Series::create(&root.join("show")).unwrap();
    let path = root.join("Show - 01.ass");
    fs::copy(raw_subtitle(), &path).unwrap();
    let records = series.read_files(&[path]).unwrap();
    series.add_sources(&records).unwrap();
    series
}

fn all_rows(series: &Series) -> Vec<SuggestionRow> {
    series
        .suggestions(&SuggestionFilter {
            status: None,
            kind: None,
        })
        .unwrap()
}

#[test]
fn create_prompt_has_the_transcript_and_the_labels() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());

    let prompt = series.create_prompt(PromptKind::Speakers, 1).unwrap();

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
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());

    let error = series.create_prompt(PromptKind::Speakers, 2).unwrap_err();

    assert!(format!("{error:#}").contains("no Ja subtitle"));
}

#[test]
fn submit_answer_creates_suggestions_and_rejects_a_second_answer() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());
    let prompt = series.create_prompt(PromptKind::Speakers, 1).unwrap();

    let count = series.submit_answer(prompt.id, ANSWER).unwrap();
    let again = series.submit_answer(prompt.id, ANSWER).unwrap_err();

    assert_eq!(count, 6);
    assert!(format!("{again:#}").contains("prompt 1 already has an answer"));
}

#[test]
fn invalid_answer_writes_no_file() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());
    let prompt = series.create_prompt(PromptKind::Speakers, 1).unwrap();

    let error = series.submit_answer(prompt.id, UNKNOWN_REF).unwrap_err();

    assert!(format!("{error:#}").contains("unknown reference C9"));
    assert!(!root.path().join("show/answers/1.json").exists());
}

#[test]
fn approve_all_shown_makes_speakers_known() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());
    let prompt = series.create_prompt(PromptKind::Speakers, 1).unwrap();
    series.submit_answer(prompt.id, ANSWER).unwrap();
    let before = series.progress().unwrap()[0].unknown_lines.unwrap();
    let pending = series
        .suggestions(&SuggestionFilter {
            status: Some(Status::Pending),
            kind: None,
        })
        .unwrap();
    let ids: Vec<SuggestionId> = pending.iter().map(|row| row.id.clone()).collect();

    let decided = series.decide(&ids, Action::Approve).unwrap();

    assert_eq!((decided.applied, decided.skipped), (6, 0));
    let after = series.progress().unwrap()[0].unknown_lines.unwrap();
    assert!(
        after < before,
        "unknown lines went from {before} to {after}"
    );
}

#[test]
fn a_rejected_character_makes_its_label_invalid() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());
    let prompt = series.create_prompt(PromptKind::Speakers, 1).unwrap();
    series.submit_answer(prompt.id, ANSWER).unwrap();

    series
        .decide(&[SuggestionId("1-1".to_string())], Action::Reject)
        .unwrap();

    let label = all_rows(&series)
        .into_iter()
        .find(|row| row.id.0 == "1-2")
        .unwrap();
    assert_eq!(label.status, Status::Invalid);
}

#[test]
fn a_label_waits_while_its_character_is_pending() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());
    let prompt = series.create_prompt(PromptKind::Speakers, 1).unwrap();
    series.submit_answer(prompt.id, ANSWER).unwrap();

    let decided = series
        .decide(&[SuggestionId("1-2".to_string())], Action::Approve)
        .unwrap();

    assert_eq!((decided.applied, decided.skipped), (0, 1));
    let label = all_rows(&series)
        .into_iter()
        .find(|row| row.id.0 == "1-2")
        .unwrap();
    assert!(label.blocked);
    assert_eq!(label.status, Status::Pending);
}

#[test]
fn revoke_returns_an_approved_character_to_pending() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());
    let prompt = series.create_prompt(PromptKind::Speakers, 1).unwrap();
    series.submit_answer(prompt.id, ANSWER).unwrap();
    series
        .decide(&[SuggestionId("1-1".to_string())], Action::Approve)
        .unwrap();

    series
        .decide(&[SuggestionId("1-1".to_string())], Action::Revoke)
        .unwrap();

    let character = all_rows(&series)
        .into_iter()
        .find(|row| row.id.0 == "1-1")
        .unwrap();
    assert_eq!(character.status, Status::Pending);
}

#[test]
fn revoking_a_line_speaker_brings_back_its_unknown_line() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());
    let prompt = series.create_prompt(PromptKind::Speakers, 1).unwrap();
    series.submit_answer(prompt.id, ANSWER).unwrap();
    let ids: Vec<SuggestionId> = all_rows(&series).into_iter().map(|row| row.id).collect();
    series.decide(&ids, Action::Approve).unwrap();
    let approved = series.progress().unwrap()[0].unknown_lines.unwrap();

    series
        .decide(&[SuggestionId("1-5".to_string())], Action::Revoke)
        .unwrap();

    let revoked = series.progress().unwrap()[0].unknown_lines.unwrap();
    assert!(
        revoked > approved,
        "unknown lines went from {approved} to {revoked}"
    );
}

#[test]
fn a_second_approval_of_the_same_label_is_skipped() {
    let root = TempDir::new().unwrap();
    let series = series_with_episode_one(root.path());
    let first = series.create_prompt(PromptKind::Speakers, 1).unwrap();
    series.submit_answer(first.id, ANSWER).unwrap();
    series
        .decide(
            &[
                SuggestionId("1-1".to_string()),
                SuggestionId("1-2".to_string()),
            ],
            Action::Approve,
        )
        .unwrap();
    let second = series.create_prompt(PromptKind::Speakers, 1).unwrap();
    series.submit_answer(second.id, ANSWER).unwrap();

    let decided = series
        .decide(&[SuggestionId("2-2".to_string())], Action::Approve)
        .unwrap();

    assert_eq!((decided.applied, decided.skipped), (0, 1));
}
