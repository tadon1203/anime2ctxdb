mod common;

use anime2ctxdb::{Importance, PromptKind, Series};
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

const TWO_ENTITIES: &str = r#"{
  "new_entities": [
    { "kind": "place", "name": "教室", "evidence": ["L1"] },
    { "kind": "place", "name": "屋上", "importance": "main", "evidence": ["L1"] }
  ],
  "facts": [{ "entities": ["教室"], "text": "教室の事実", "evidence": ["L3"] }]
}"#;

const PROMOTE: &str = r#"{ "importance": [{ "name": "教室", "importance": "main" }] }"#;

fn entity_importance(series: &Series, name: &str) -> (Importance, bool) {
    let data = series.data().expect("the data loads");
    let row = data
        .entities
        .iter()
        .find(|entity| entity.name == name)
        .expect("the entity exists");
    (row.importance, row.manual)
}

#[test]
fn a_prompt_leaves_out_a_minor_entity_that_did_not_appear_in_the_last_episode() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 3);
    series
        .submit_answer(PromptKind::Story, 1, TWO_ENTITIES)
        .expect("the answer is submitted");

    let next = series
        .prompt(PromptKind::Story, 2)
        .expect("the prompt is made");
    let later = series
        .prompt(PromptKind::Story, 3)
        .expect("the prompt is made");

    assert!(next.known.contains("教室"), "it appeared in episode 1");
    assert!(next.text.contains("教室の事実"));
    assert!(later.known.contains("屋上"), "a main entity stays");
    assert!(!later.known.contains("教室"), "a minor entity is left out");
    assert!(!later.text.contains("教室の事実"), "its fact is left out");
}

#[test]
fn a_later_answer_can_raise_the_importance_of_an_entity() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 3);
    series
        .submit_answer(PromptKind::Story, 1, TWO_ENTITIES)
        .expect("the answer is submitted");

    series
        .submit_answer(PromptKind::Story, 2, PROMOTE)
        .expect("the answer is submitted");

    let later = series
        .prompt(PromptKind::Story, 3)
        .expect("the prompt is made");
    assert!(later.known.contains("教室"));
    assert_eq!(
        entity_importance(&series, "教室"),
        (Importance::Main, false)
    );
}

#[test]
fn a_hand_setting_wins_over_the_answers_and_auto_removes_it() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 3);
    series
        .submit_answer(PromptKind::Story, 1, TWO_ENTITIES)
        .expect("the answer is submitted");
    series
        .submit_answer(PromptKind::Story, 2, PROMOTE)
        .expect("the answer is submitted");

    series
        .set_importance("教室", Some(Importance::Minor))
        .expect("the importance is set");

    assert_eq!(
        entity_importance(&series, "教室"),
        (Importance::Minor, true)
    );
    assert!(root.path().join("show/importance.json").exists());
    let later = series
        .prompt(PromptKind::Story, 3)
        .expect("the prompt is made");
    assert!(!later.known.contains("教室"));

    series
        .set_importance("教室", None)
        .expect("the setting is removed");

    assert_eq!(
        entity_importance(&series, "教室"),
        (Importance::Main, false)
    );
}

#[test]
fn a_hand_setting_needs_an_entity() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    let error = series
        .set_importance("誰か", Some(Importance::Main))
        .expect_err("no answer introduces the entity");

    assert!(format!("{error:#}").contains("unknown entity 誰か"));
    assert!(!root.path().join("show/importance.json").exists());
}

#[test]
fn a_scene_runs_to_the_start_of_the_next_scene() {
    let root = TempDir::new().expect("a temporary directory");
    let series = series_with_episodes(root.path(), 1);

    series
        .submit_answer(
            PromptKind::Story,
            1,
            r#"{ "scenes": [{ "from": "L1", "summary": "一" }, { "from": "L5", "summary": "二" }] }"#,
        )
        .expect("the answer is submitted");

    let scenes = series.data().expect("the data loads").scenes;
    assert_eq!(scenes[0].end_ms, scenes[1].start_ms);
    assert!(
        scenes[1].end_ms > scenes[1].start_ms,
        "the last scene runs on"
    );
}
