mod common;

use anime2ctxdb::PromptKind;
use common::series_with_episodes;
use tempfile::TempDir;

const STORY_ANSWER: &str = r#"{
  "new_entities": [{ "kind": "place", "name": "教室", "evidence": ["L1"] }],
  "facts": [{ "entities": ["教室"], "text": "教室で話す", "evidence": ["L3"] }]
}"#;

const NO_EVIDENCE: &str = r#"{
  "facts": [{ "entities": [], "text": "何もない", "evidence": [] }]
}"#;

const SECOND_EPISODE: &str = r#"{
  "facts": [{ "entities": ["教室"], "text": "また教室にいる", "evidence": ["L2"] }]
}"#;

const OTHER_PLACE: &str = r#"{
  "new_entities": [{ "kind": "place", "name": "屋上", "evidence": ["L1"] }]
}"#;

#[test]
fn a_story_prompt_shows_the_entities_and_facts_from_earlier_episodes() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    let first = series
        .prompt(PromptKind::Story, 1)
        .expect("the prompt is made");
    assert!(
        first
            .text
            .contains("Known facts from earlier episodes:\nnone\n")
    );
    series
        .submit_answer(PromptKind::Story, 1, STORY_ANSWER)
        .expect("the answer is submitted");

    let second = series
        .prompt(PromptKind::Story, 2)
        .expect("the prompt is made");

    assert_eq!(second.kind, PromptKind::Story);
    assert!(second.text.contains("教室 (place)"));
    assert!(second.text.contains("[教室] 教室で話す"));
}

#[test]
fn an_answer_gives_entities_and_facts_at_once() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    let count = series
        .submit_answer(PromptKind::Story, 1, STORY_ANSWER)
        .expect("the answer is submitted");

    assert_eq!(count, 2);
    let data = series.data().expect("the data loads");
    assert_eq!(data.entities[0].name, "教室");
    assert_eq!(data.entities[0].kind, "place");
    assert_eq!(data.facts[0].text, "教室で話す");
    assert_eq!(data.facts[0].evidence.len(), 1);
}

#[test]
fn a_new_answer_replaces_the_old_one_whole() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);
    series
        .submit_answer(PromptKind::Story, 1, STORY_ANSWER)
        .expect("the answer is submitted");

    series
        .submit_answer(PromptKind::Story, 1, OTHER_PLACE)
        .expect("the answer is replaced");

    let data = series.data().expect("the data loads");
    let names: Vec<&str> = data
        .entities
        .iter()
        .map(|entity| entity.name.as_str())
        .collect();
    assert_eq!(names, ["屋上"]);
    assert_eq!(data.facts.len(), 0);
}

#[test]
fn a_name_that_loses_its_entity_stays_as_text_in_a_fact() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 2);
    series
        .submit_answer(PromptKind::Story, 1, STORY_ANSWER)
        .expect("the answer is submitted");
    series
        .submit_answer(PromptKind::Story, 2, SECOND_EPISODE)
        .expect("the answer is submitted");

    series
        .submit_answer(PromptKind::Story, 1, OTHER_PLACE)
        .expect("the answer is replaced");

    let data = series.data().expect("the data loads");
    assert_eq!(data.facts.len(), 1);
    assert_eq!(data.facts[0].entities, ["教室"]);
    assert!(data.entities.iter().all(|entity| entity.name != "教室"));
}

#[test]
fn an_unknown_entity_writes_no_file() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    let error = series
        .submit_answer(PromptKind::Story, 1, SECOND_EPISODE)
        .expect_err("the entity is not known");

    assert!(format!("{error:#}").contains("unknown entity 教室"));
    assert!(!root.path().join("show/answers/story-1.json").exists());
}

#[test]
fn a_fact_without_evidence_writes_no_answer_file() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    let error = series
        .submit_answer(PromptKind::Story, 1, NO_EVIDENCE)
        .expect_err("the fact has no evidence");

    assert!(format!("{error:#}").contains("entry 1 of facts has no evidence"));
    assert!(!root.path().join("show/answers/story-1.json").exists());
}
