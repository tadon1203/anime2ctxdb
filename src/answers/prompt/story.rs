use super::{
    AnswerError, Prompt, PromptKind, PromptLine, entity_list, entity_name, evidence_of,
    line_number, line_ref, list_or_none, reference, speaker_text,
};
use crate::answers::answer::{Answer, Fact, ImportanceChange, NewEntity, Scene};
use crate::importance::Importance;
use crate::model::{Entity, Position, format_clock};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

const TEMPLATE: &str = include_str!("../../../prompts/story.md");

const FORMAT: &str = r#"{
  "new_entities": [{ "kind": "place", "name": "...", "reading": "...", "importance": "minor", "evidence": ["L3"] }],
  "importance": [{ "name": "...", "importance": "main" }],
  "facts": [{ "entities": ["name", "name"], "text": "...", "evidence": ["L12"] }],
  "scenes": [{ "from": "L1", "place": "name", "summary": "..." }]
}"#;

/// What a Story prompt needs from the series.
pub struct StoryInput {
    pub lines: Vec<PromptLine>,
    /// The entities that are known at the episode, of all kinds.
    pub entities: Vec<Entity>,
    /// The known facts from earlier episodes, in time order.
    pub facts: Vec<PromptFact>,
}

/// A known fact, as the prompt shows it.
pub struct PromptFact {
    pub since: Position,
    /// The names of the entities of the fact.
    pub entities: Vec<String>,
    pub text: String,
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    new_entities: Vec<ReplyEntity>,
    #[serde(default)]
    facts: Vec<ReplyFact>,
    #[serde(default)]
    scenes: Vec<ReplyScene>,
    #[serde(default)]
    importance: Vec<ReplyImportance>,
}

#[derive(Deserialize)]
struct ReplyEntity {
    kind: String,
    name: String,
    reading: Option<String>,
    importance: Option<Importance>,
    evidence: Vec<String>,
}

#[derive(Deserialize)]
struct ReplyImportance {
    name: String,
    importance: Importance,
}

#[derive(Deserialize)]
struct ReplyFact {
    entities: Vec<String>,
    text: String,
    evidence: Vec<String>,
}

#[derive(Deserialize)]
struct ReplyScene {
    from: String,
    place: Option<String>,
    summary: String,
}

/// Builds a Story prompt for one episode.
pub fn make(episode: u32, input: &StoryInput) -> Prompt {
    let mut refs = BTreeMap::new();
    let entity_lines = entity_list(&input.entities);
    let transcript_lines = transcript(&input.lines, &mut refs);
    let fact_lines: Vec<String> = input.facts.iter().map(fact_text).collect();

    let text = TEMPLATE
        .replace("{{entities}}", &list_or_none(&entity_lines))
        .replace("{{facts}}", &list_or_none(&fact_lines))
        .replace("{{episode}}", &format!("episode {episode}"))
        .replace("{{transcript}}", &list_or_none(&transcript_lines))
        .replace("{{format}}", FORMAT);

    Prompt {
        kind: PromptKind::Story,
        episode,
        text,
        refs,
        known: input
            .entities
            .iter()
            .map(|entity| entity.name.clone())
            .collect(),
    }
}

/// Writes the lines of the episode, for example `L12 [明智あんな] text`.
fn transcript(lines: &[PromptLine], refs: &mut BTreeMap<String, String>) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            format!(
                "{} {} {}",
                reference(line, refs),
                speaker_text(&line.speaker),
                line.text
            )
        })
        .collect()
}

/// One known fact, for example `E1 03:50 [明智あんな, 坂本みくる] text`.
fn fact_text(fact: &PromptFact) -> String {
    format!(
        "E{} {} [{}] {}",
        fact.since.episode,
        format_clock(fact.since.time_ms),
        fact.entities.join(", "),
        fact.text
    )
}

/// Makes the answer from the reply. An entity that the reply introduces can be used in it.
pub(super) fn parse(prompt: &Prompt, json: &str) -> Result<Answer, AnswerError> {
    let reply: Reply = serde_json::from_str(json)?;
    let declared: BTreeSet<String> = reply
        .new_entities
        .iter()
        .map(|entity| entity.name.clone())
        .collect();

    let mut entities = Vec::new();
    for (index, entity) in reply.new_entities.into_iter().enumerate() {
        entities.push(NewEntity {
            evidence: evidence_of(prompt, "new_entities", index, &entity.evidence)?,
            kind: entity.kind,
            name: entity.name,
            reading: entity.reading,
            importance: entity.importance,
        });
    }

    let mut importance = Vec::new();
    for change in reply.importance {
        importance.push(ImportanceChange {
            name: entity_name(prompt, &declared, &change.name)?,
            importance: change.importance,
        });
    }

    let mut facts = Vec::new();
    for (index, fact) in reply.facts.into_iter().enumerate() {
        facts.push(Fact {
            evidence: evidence_of(prompt, "facts", index, &fact.evidence)?,
            entities: fact
                .entities
                .iter()
                .map(|name| entity_name(prompt, &declared, name))
                .collect::<Result<Vec<String>, _>>()?,
            text: fact.text,
        });
    }

    let mut scenes = Vec::new();
    let mut previous = 0;
    for (index, scene) in reply.scenes.into_iter().enumerate() {
        let start = line_ref(prompt, &scene.from)?;
        let number = line_number(&scene.from);
        if number <= previous {
            return Err(AnswerError::SceneOrder(index + 1));
        }
        previous = number;
        scenes.push(Scene {
            start,
            place: scene
                .place
                .as_deref()
                .map(|name| entity_name(prompt, &declared, name))
                .transpose()?,
            summary: scene.summary,
        });
    }

    Ok(Answer::Story {
        episode: prompt.episode,
        entities,
        facts,
        scenes,
        importance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::ass::LineId;

    fn prompt() -> Prompt {
        Prompt {
            kind: PromptKind::Story,
            episode: 2,
            text: String::new(),
            refs: BTreeMap::from([
                ("L1".to_string(), "s:0".to_string()),
                ("L2".to_string(), "s:1".to_string()),
                ("L3".to_string(), "s:2".to_string()),
            ]),
            known: BTreeSet::from(["坂本みくる".to_string(), "職員室".to_string()]),
        }
    }

    const ANSWER: &str = r#"{
      "new_entities": [{ "kind": "place", "name": "教室", "evidence": ["L1"] }],
      "facts": [{ "entities": ["坂本みくる", "教室"], "text": "t", "evidence": ["L2", "L3"] }],
      "scenes": [{ "from": "L1", "place": "教室", "summary": "s" }]
    }"#;

    fn answer(json: &str) -> Result<Answer, AnswerError> {
        super::super::parse_answer(&prompt(), json)
    }

    #[test]
    fn the_answer_has_entities_facts_and_scenes_with_real_ids() {
        let answer = answer(ANSWER).expect("the answer parses");

        let Answer::Story {
            episode,
            entities,
            facts,
            scenes,
            ..
        } = answer
        else {
            panic!("a story answer");
        };
        assert_eq!(episode, 2);
        assert_eq!(entities[0].name, "教室");
        assert_eq!(facts[0].entities, ["坂本みくる", "教室"]);
        assert_eq!(
            facts[0].evidence.lines(),
            [LineId("s:1".to_string()), LineId("s:2".to_string())]
        );
        assert_eq!(scenes[0].start, LineId("s:0".to_string()));
        assert_eq!(scenes[0].place.as_deref(), Some("教室"));
    }

    #[test]
    fn an_entity_can_be_a_known_one_of_any_kind() {
        let answer =
            answer(r#"{ "facts": [{ "entities": ["職員室"], "text": "t", "evidence": ["L1"] }] }"#)
                .expect("the answer parses");

        assert!(matches!(
            &answer,
            Answer::Story { facts, .. } if facts[0].entities == ["職員室"]
        ));
    }

    #[test]
    fn an_unknown_entity_is_an_error() {
        let error =
            answer(r#"{ "facts": [{ "entities": ["誰か"], "text": "t", "evidence": ["L1"] }] }"#)
                .expect_err("the name is unknown");

        assert!(matches!(error, AnswerError::UnknownEntity(name) if name == "誰か"));
    }

    #[test]
    fn a_fact_needs_evidence() {
        let error = answer(r#"{ "facts": [{ "entities": [], "text": "t", "evidence": [] }] }"#)
            .expect_err("the evidence is missing");

        assert!(matches!(
            error,
            AnswerError::MissingEvidence {
                section: "facts",
                position: 1
            }
        ));
    }

    #[test]
    fn a_new_entity_needs_evidence() {
        let error =
            answer(r#"{ "new_entities": [{ "kind": "place", "name": "x", "evidence": [] }] }"#)
                .expect_err("the evidence is missing");

        assert!(matches!(
            error,
            AnswerError::MissingEvidence {
                section: "new_entities",
                position: 1
            }
        ));
    }

    #[test]
    fn a_scene_that_does_not_start_after_the_previous_one_is_an_error() {
        let error = answer(
            r#"{ "scenes": [{ "from": "L3", "summary": "s" }, { "from": "L1", "summary": "s" }] }"#,
        )
        .expect_err("the second scene starts first");

        assert!(matches!(error, AnswerError::SceneOrder(2)));
    }

    #[test]
    fn an_entity_can_get_an_importance_when_it_is_new_and_later() {
        let answer = answer(
            r#"{
              "new_entities": [{ "kind": "character", "name": "宇都見", "importance": "recurring", "evidence": ["L1"] }],
              "importance": [{ "name": "坂本みくる", "importance": "main" }]
            }"#,
        )
        .expect("the answer parses");

        let Answer::Story {
            entities,
            importance,
            ..
        } = answer
        else {
            panic!("a story answer");
        };
        assert_eq!(entities[0].importance, Some(Importance::Recurring));
        assert_eq!(importance[0].name, "坂本みくる");
        assert_eq!(importance[0].importance, Importance::Main);
    }

    #[test]
    fn the_importance_of_an_unknown_entity_is_an_error() {
        let error = answer(r#"{ "importance": [{ "name": "誰か", "importance": "main" }] }"#)
            .expect_err("the name is unknown");

        assert!(matches!(error, AnswerError::UnknownEntity(name) if name == "誰か"));
    }

    #[test]
    fn a_wrong_importance_is_an_error() {
        let error = answer(r#"{ "importance": [{ "name": "職員室", "importance": "huge" }] }"#)
            .expect_err("the value is wrong");

        assert!(matches!(error, AnswerError::InvalidJson(_)));
    }

    #[test]
    fn make_writes_the_transcript_with_references() {
        let line = |id: &str| PromptLine {
            id: LineId(id.to_string()),
            start_ms: 1_000,
            color: None,
            label: None,
            text: "t".to_string(),
            speaker: crate::speakers::Speaker::Unknown,
        };
        let input = StoryInput {
            lines: vec![line("s:0"), line("s:1")],
            entities: vec![],
            facts: vec![],
        };

        let prompt = make(2, &input);

        assert_eq!(prompt.episode, 2);
        assert!(prompt.text.contains("episode 2"));
        assert!(prompt.text.contains("L1 [?] t"));
        assert_eq!(prompt.refs["L2"], "s:1");
    }

    #[test]
    fn a_fact_with_no_entities_is_valid() {
        let answer =
            answer(r#"{ "facts": [{ "entities": [], "text": "t", "evidence": ["L1"] }] }"#)
                .expect("the answer parses");

        assert!(matches!(
            &answer,
            Answer::Story { facts, .. } if facts[0].entities.is_empty()
        ));
    }
}
