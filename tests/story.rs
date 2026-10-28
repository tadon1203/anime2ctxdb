mod common;

use anime2ctxdb::{
    Action, PromptKind, Series, Status, SuggestionFilter, SuggestionKind, SuggestionRow,
};
use common::{approve_all_pending, series_with_episodes};
use tempfile::TempDir;

const STORY_ANSWER: &str = r#"{
  "new_entities": [{ "ref": "N1", "kind": "place", "name": "教室", "evidence": ["L1"] }],
  "facts": [{ "entities": ["N1"], "text": "教室で話す", "evidence": ["L3"] }]
}"#;

const NO_EVIDENCE: &str = r#"{
  "facts": [{ "entities": [], "text": "何もない", "evidence": [] }]
}"#;

fn all_rows(series: &Series) -> Vec<SuggestionRow> {
    series
        .suggestions(&SuggestionFilter {
            status: None,
            kind: None,
        })
        .expect("the suggestions load")
}

fn row_of_kind(series: &Series, kind: SuggestionKind) -> SuggestionRow {
    all_rows(series)
        .into_iter()
        .find(|row| row.kind == kind)
        .expect("the answer has a suggestion of this kind")
}

#[test]
fn a_story_prompt_shows_the_entities_and_facts_from_earlier_episodes() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    let first = series
        .create_prompt(PromptKind::Story, 1)
        .expect("the prompt is created");
    assert!(
        first
            .text
            .contains("Known facts before this episode:\nnone\n")
    );
    series
        .submit_answer(first.id, STORY_ANSWER)
        .expect("the answer is submitted");
    approve_all_pending(&series);

    let second = series
        .create_prompt(PromptKind::Story, 2)
        .expect("the prompt is created");

    assert_eq!(second.kind, PromptKind::Story);
    assert!(second.text.contains("T1 教室 (place)"));
    assert!(second.text.contains("[教室] 教室で話す"));
}

#[test]
fn a_fact_waits_for_its_entity_and_is_not_known_until_it_is_approved() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    let first = series
        .create_prompt(PromptKind::Story, 1)
        .expect("the prompt is created");
    series
        .submit_answer(first.id, STORY_ANSWER)
        .expect("the answer is submitted");
    let fact = row_of_kind(&series, SuggestionKind::Fact);

    let decided = series
        .decide(std::slice::from_ref(&fact.id), Action::Approve)
        .expect("the decision is made");

    assert_eq!((decided.applied, decided.skipped), (0, 1));
    let second = series
        .create_prompt(PromptKind::Story, 2)
        .expect("the prompt is created");
    assert!(
        second
            .text
            .contains("Known facts before this episode:\nnone\n")
    );
}

#[test]
fn rejecting_an_entity_makes_its_fact_invalid() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);
    let first = series
        .create_prompt(PromptKind::Story, 1)
        .expect("the prompt is created");
    series
        .submit_answer(first.id, STORY_ANSWER)
        .expect("the answer is submitted");
    let entity = row_of_kind(&series, SuggestionKind::Entity);

    series
        .decide(std::slice::from_ref(&entity.id), Action::Reject)
        .expect("the decision is made");

    assert_eq!(
        row_of_kind(&series, SuggestionKind::Fact).status,
        Status::Invalid
    );
}

#[test]
fn a_fact_without_evidence_writes_no_answer_file() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);
    let first = series
        .create_prompt(PromptKind::Story, 1)
        .expect("the prompt is created");

    let error = series
        .submit_answer(first.id, NO_EVIDENCE)
        .expect_err("the fact has no evidence");

    assert!(format!("{error:#}").contains("entry 1 of facts has no evidence"));
    assert!(!root.path().join("show/answers/1.json").exists());
}
