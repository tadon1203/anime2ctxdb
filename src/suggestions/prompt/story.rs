use super::{
    AnswerError, Numbering, Prompt, PromptId, PromptKind, PromptLine, entity_list, entity_ref,
    evidence_of, line_ref, list_or_none, transcript,
};
use crate::model::{Entity, EntityId, Position, format_clock};
use crate::suggestions::suggestion::{Content, Suggestion};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

const TEMPLATE: &str = include_str!("../../../prompts/story.md");

const FORMAT: &str = r#"{
  "new_entities": [{ "ref": "N1", "kind": "place", "name": "...", "reading": "...", "evidence": ["L3"] }],
  "facts": [{ "entities": ["C1", "N1"], "text": "...", "evidence": ["L12", "L15"] }],
  "scenes": [{ "start": "L1", "end": "L40", "place": "N1", "summary": "..." }]
}"#;

/// What a Story prompt needs from the series.
pub struct StoryInput {
    pub lines: Vec<PromptLine>,
    /// All approved entities, of all kinds.
    pub entities: Vec<Entity>,
    /// The approved facts from earlier episodes, in time order.
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
struct Answer {
    #[serde(default)]
    new_entities: Vec<NewEntity>,
    #[serde(default)]
    facts: Vec<AnswerFact>,
    #[serde(default)]
    scenes: Vec<AnswerScene>,
}

#[derive(Deserialize)]
struct NewEntity {
    #[serde(rename = "ref")]
    reference: String,
    kind: String,
    name: String,
    reading: Option<String>,
    evidence: Vec<String>,
}

#[derive(Deserialize)]
struct AnswerFact {
    entities: Vec<String>,
    text: String,
    evidence: Vec<String>,
}

#[derive(Deserialize)]
struct AnswerScene {
    start: String,
    end: String,
    place: Option<String>,
    summary: String,
}

/// Builds a Story prompt for one episode.
pub fn make(id: PromptId, episode: u32, input: &StoryInput) -> Prompt {
    let mut refs = BTreeMap::new();
    let (entity_lines, entity_refs) = entity_list(&input.entities, &mut refs);
    let transcript_lines = transcript(&input.lines, &entity_refs, &mut refs);
    let fact_lines: Vec<String> = input.facts.iter().map(fact_text).collect();

    let text = TEMPLATE
        .replace("{{entities}}", &list_or_none(&entity_lines))
        .replace("{{facts}}", &list_or_none(&fact_lines))
        .replace("{{episode}}", &episode.to_string())
        .replace("{{transcript}}", &list_or_none(&transcript_lines))
        .replace("{{format}}", FORMAT);

    Prompt {
        id,
        kind: PromptKind::Story,
        episode,
        text,
        refs,
    }
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

/// Makes the suggestions of a Story answer. The order is: new entities, facts, then scenes.
pub(super) fn parse(prompt: &Prompt, json: &str) -> Result<Vec<Suggestion>, AnswerError> {
    let answer: Answer = serde_json::from_str(json)?;
    let mut numbering = Numbering::new(prompt.id);
    let mut suggestions = Vec::new();

    let mut new_refs: HashMap<String, EntityId> = HashMap::new();
    for (index, entity) in answer.new_entities.into_iter().enumerate() {
        let evidence = evidence_of(prompt, "new_entities", index, &entity.evidence)?;
        let id = numbering.next_id();
        new_refs.insert(entity.reference, EntityId(id.0.clone()));
        suggestions.push(Suggestion {
            id,
            content: Content::Entity {
                kind: entity.kind,
                name: entity.name,
                reading: entity.reading,
                evidence,
            },
        });
    }

    for (index, fact) in answer.facts.into_iter().enumerate() {
        let evidence = evidence_of(prompt, "facts", index, &fact.evidence)?;
        let entities = fact
            .entities
            .iter()
            .map(|reference| entity_ref(prompt, &new_refs, reference).map(|id| id.0))
            .collect::<Result<Vec<String>, _>>()?;
        suggestions.push(Suggestion {
            id: numbering.next_id(),
            content: Content::Fact {
                entities,
                text: fact.text,
                evidence,
            },
        });
    }

    for (index, scene) in answer.scenes.into_iter().enumerate() {
        let start = line_ref(prompt, &scene.start)?;
        let end = line_ref(prompt, &scene.end)?;
        if line_number(&scene.end) < line_number(&scene.start) {
            return Err(AnswerError::SceneOrder(index + 1));
        }
        let place = scene
            .place
            .as_deref()
            .map(|reference| entity_ref(prompt, &new_refs, reference).map(|id| id.0))
            .transpose()?;
        suggestions.push(Suggestion {
            id: numbering.next_id(),
            content: Content::Scene {
                start,
                end,
                place,
                summary: scene.summary,
            },
        });
    }

    Ok(suggestions)
}

/// The number of an `L` reference. Only a reference that `line_ref` accepted comes here.
fn line_number(reference: &str) -> u32 {
    reference[1..]
        .parse()
        .expect("a line reference has a number")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::ass::LineId;
    use crate::suggestions::suggestion::Evidence;

    fn prompt() -> Prompt {
        Prompt {
            id: PromptId(5),
            kind: PromptKind::Story,
            episode: 2,
            text: String::new(),
            refs: BTreeMap::from([
                ("L1".to_string(), "s:0".to_string()),
                ("L2".to_string(), "s:1".to_string()),
                ("L3".to_string(), "s:2".to_string()),
                ("C1".to_string(), "2-1".to_string()),
                ("T1".to_string(), "3-1".to_string()),
            ]),
        }
    }

    const ANSWER: &str = r#"{
      "new_entities": [{ "ref": "N1", "kind": "place", "name": "教室", "evidence": ["L1"] }],
      "facts": [{ "entities": ["C1", "N1"], "text": "t", "evidence": ["L2", "L3"] }],
      "scenes": [{ "start": "L1", "end": "L3", "place": "N1", "summary": "s" }]
    }"#;

    fn answer(json: &str) -> Result<Vec<Suggestion>, AnswerError> {
        super::super::parse_answer(&prompt(), json)
    }

    #[test]
    fn new_entities_come_first_then_facts_then_scenes() {
        let suggestions = answer(ANSWER).expect("the answer parses");

        let ids: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.id.0.as_str())
            .collect();
        assert_eq!(ids, ["5-1", "5-2", "5-3"]);
        assert_eq!(
            suggestions[1].content,
            Content::Fact {
                entities: vec!["2-1".to_string(), "5-1".to_string()],
                text: "t".to_string(),
                evidence: Evidence::new(
                    vec![LineId("s:1".to_string()), LineId("s:2".to_string()),]
                )
                .expect("two lines"),
            }
        );
    }

    #[test]
    fn an_entity_reference_can_be_a_character_or_another_entity() {
        let suggestions = answer(
            r#"{ "facts": [{ "entities": ["C1", "T1"], "text": "t", "evidence": ["L1"] }] }"#,
        )
        .expect("the answer parses");

        assert!(matches!(
            &suggestions[0].content,
            Content::Fact { entities, .. } if entities == &["2-1", "3-1"]
        ));
    }

    #[test]
    fn an_unknown_reference_is_an_error() {
        let error =
            answer(r#"{ "facts": [{ "entities": ["C9"], "text": "t", "evidence": ["L1"] }] }"#)
                .expect_err("C9 is unknown");

        assert!(matches!(error, AnswerError::UnknownRef(reference) if reference == "C9"));
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
        let error = answer(r#"{ "new_entities": [{ "ref": "N1", "kind": "place", "name": "x", "evidence": [] }] }"#)
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
    fn a_scene_that_ends_before_it_starts_is_an_error() {
        let error = answer(
            r#"{ "scenes": [{ "start": "L3", "end": "L1", "place": null, "summary": "s" }] }"#,
        )
        .expect_err("the end is before the start");

        assert!(matches!(error, AnswerError::SceneOrder(1)));
    }

    #[test]
    fn a_fact_with_no_entities_is_valid() {
        let suggestions =
            answer(r#"{ "facts": [{ "entities": [], "text": "t", "evidence": ["L1"] }] }"#)
                .expect("the answer parses");

        assert!(matches!(
            &suggestions[0].content,
            Content::Fact { entities, .. } if entities.is_empty()
        ));
    }
}
