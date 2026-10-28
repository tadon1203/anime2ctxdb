use crate::model::{Entity, EntityId, Position, format_clock};
use std::collections::{HashMap, HashSet};

/// Everything the package can show at one position. The index reads it, and `build` only sorts it.
pub struct PackageInput {
    pub at: Position,
    /// The entities whose `since` is at or before `at`, in `since` order.
    pub entities: Vec<Entity>,
    /// The facts whose `since` is at or before `at`, in `since` order.
    pub facts: Vec<PackageFact>,
    /// The scenes whose start is at or before `at`. This includes a scene in progress.
    pub scenes: Vec<PackageScene>,
    /// The lines of `at.episode` that end at or before `at`, in time order.
    pub lines: Vec<PackageLine>,
}

pub struct PackageFact {
    pub since: Position,
    pub entities: Vec<EntityId>,
    pub text: String,
    /// The lines of the fact, in time order.
    pub evidence: Vec<PackageLine>,
}

pub struct PackageScene {
    pub episode: u32,
    pub start_ms: u32,
    pub end_ms: u32,
    pub place: Option<EntityId>,
    pub summary: String,
}

pub struct PackageLine {
    pub episode: u32,
    pub start_ms: u32,
    pub text: String,
    /// Only the speakers from a label, a color, or an AI answer. A copied speaker is not here.
    pub speakers: Vec<EntityId>,
}

/// A node of the YAML document. The renderer writes it, so every text is quoted.
enum Node {
    Text(String),
    Number(u32),
    Map(Vec<(&'static str, Node)>),
    List(Vec<Node>),
}

/// Makes the context package as YAML text.
///
/// The package holds only what the story shows at `input.at`. A name of an entity that is not
/// in `input.entities` is left out, so a later name does not leak.
pub fn build(input: &PackageInput) -> String {
    let names: HashMap<&EntityId, &str> = input
        .entities
        .iter()
        .map(|entity| (&entity.id, entity.name.as_str()))
        .collect();
    let at = input.at;

    let mut ended: Vec<&PackageScene> = input
        .scenes
        .iter()
        .filter(|scene| (scene.episode, scene.end_ms) <= (at.episode, at.time_ms))
        .collect();
    ended.sort_by_key(|scene| (scene.episode, scene.start_ms));

    let current = input
        .scenes
        .iter()
        .filter(|scene| {
            scene.episode == at.episode && scene.start_ms <= at.time_ms && at.time_ms < scene.end_ms
        })
        .max_by_key(|scene| scene.start_ms);

    // The current lines start at the current scene. Without one, they start after the last ended scene.
    let from_ms = match current {
        Some(scene) => scene.start_ms,
        None => ended
            .iter()
            .rev()
            .find(|scene| scene.episode == at.episode)
            .map_or(0, |scene| scene.end_ms),
    };
    let current_lines: Vec<&PackageLine> = input
        .lines
        .iter()
        .filter(|line| line.start_ms >= from_ms)
        .collect();

    let mut speakers: Vec<&str> = Vec::new();
    let mut seen: HashSet<&EntityId> = HashSet::new();
    for line in &current_lines {
        for speaker in &line.speakers {
            if let Some(name) = names.get(speaker)
                && seen.insert(speaker)
            {
                speakers.push(name);
            }
        }
    }

    let mut current_scene = vec![];
    if let Some(place) = current
        .and_then(|scene| scene.place.as_ref())
        .and_then(|id| names.get(id))
    {
        current_scene.push(("place", text(place)));
    }
    current_scene.push((
        "speakers",
        Node::List(speakers.into_iter().map(text).collect()),
    ));
    current_scene.push((
        "lines",
        Node::List(
            current_lines
                .iter()
                .map(|line| line_node(line, &names))
                .collect(),
        ),
    ));

    let document = Node::Map(vec![
        (
            "position",
            Node::Map(vec![
                ("episode", Node::Number(at.episode)),
                ("time", text(&format_clock(at.time_ms))),
            ]),
        ),
        (
            "entities",
            Node::List(input.entities.iter().map(entity_node).collect()),
        ),
        (
            "scenes",
            Node::List(
                ended
                    .iter()
                    .map(|scene| scene_node(scene, &names))
                    .collect(),
            ),
        ),
        (
            "facts",
            Node::List(
                input
                    .facts
                    .iter()
                    .map(|fact| fact_node(fact, &names))
                    .collect(),
            ),
        ),
        ("current_scene", Node::Map(current_scene)),
    ]);

    let mut lines = block(&document, 0);
    lines.push(String::new());
    lines.join("\n")
}

fn text(value: &str) -> Node {
    Node::Text(value.to_string())
}

/// A name of an entity, when the input has the entity.
fn name_node(id: &EntityId, names: &HashMap<&EntityId, &str>) -> Option<Node> {
    names.get(id).map(|name| text(name))
}

fn entity_node(entity: &Entity) -> Node {
    let mut fields = vec![("name", text(&entity.name))];
    if let Some(reading) = &entity.reading {
        fields.push(("reading", text(reading)));
    }
    fields.push(("kind", text(&entity.kind)));
    Node::Map(fields)
}

fn scene_node(scene: &PackageScene, names: &HashMap<&EntityId, &str>) -> Node {
    let mut fields = vec![
        ("episode", Node::Number(scene.episode)),
        ("start", text(&format_clock(scene.start_ms))),
        ("end", text(&format_clock(scene.end_ms))),
    ];
    if let Some(place) = scene.place.as_ref().and_then(|id| name_node(id, names)) {
        fields.push(("place", place));
    }
    fields.push(("summary", text(&scene.summary)));
    Node::Map(fields)
}

fn fact_node(fact: &PackageFact, names: &HashMap<&EntityId, &str>) -> Node {
    let entities = fact
        .entities
        .iter()
        .filter_map(|id| name_node(id, names))
        .collect();
    Node::Map(vec![
        ("episode", Node::Number(fact.since.episode)),
        ("time", text(&format_clock(fact.since.time_ms))),
        ("entities", Node::List(entities)),
        ("text", text(&fact.text)),
        (
            "lines",
            Node::List(
                fact.evidence
                    .iter()
                    .map(|line| line_node(line, names))
                    .collect(),
            ),
        ),
    ])
}

/// A line with its speaker. A speaker is one name, or a list when there are more.
fn line_node(line: &PackageLine, names: &HashMap<&EntityId, &str>) -> Node {
    let speakers: Vec<Node> = line
        .speakers
        .iter()
        .filter_map(|id| name_node(id, names))
        .collect();
    let mut fields = vec![
        ("episode", Node::Number(line.episode)),
        ("time", text(&format_clock(line.start_ms))),
    ];
    match speakers.len() {
        0 => {}
        1 => fields.push(("speaker", speakers.into_iter().next().expect("one speaker"))),
        _ => fields.push(("speaker", Node::List(speakers))),
    }
    fields.push(("text", text(&line.text)));
    Node::Map(fields)
}

/// A node that is written on one line: a text, a number, or a list or a map of them.
fn is_inline(node: &Node) -> bool {
    match node {
        Node::Text(_) | Node::Number(_) => true,
        Node::Map(entries) => entries.iter().all(|(_, value)| is_flat(value)),
        Node::List(items) => items.iter().all(is_flat),
    }
}

/// A text, a number, or a list of them. A map value can be one of these.
fn is_flat(node: &Node) -> bool {
    match node {
        Node::Text(_) | Node::Number(_) => true,
        Node::List(items) => items
            .iter()
            .all(|item| matches!(item, Node::Text(_) | Node::Number(_))),
        Node::Map(_) => false,
    }
}

fn inline(node: &Node) -> String {
    match node {
        Node::Text(value) => serde_json::to_string(value).expect("a string is valid JSON"),
        Node::Number(number) => number.to_string(),
        Node::Map(entries) if entries.is_empty() => "{}".to_string(),
        Node::Map(entries) => {
            let fields: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{key}: {}", inline(value)))
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
        Node::List(items) => {
            let values: Vec<String> = items.iter().map(inline).collect();
            format!("[{}]", values.join(", "))
        }
    }
}

/// Writes a node as block YAML, with 2 spaces for each level.
fn block(node: &Node, indent: usize) -> Vec<String> {
    let pad = " ".repeat(indent);
    match node {
        Node::Map(entries) => entries
            .iter()
            .flat_map(|(key, value)| {
                if is_inline(value) {
                    vec![format!("{pad}{key}: {}", inline(value))]
                } else {
                    let mut lines = vec![format!("{pad}{key}:")];
                    lines.extend(block(value, indent + 2));
                    lines
                }
            })
            .collect(),
        Node::List(items) => items
            .iter()
            .flat_map(|item| {
                if is_inline(item) {
                    vec![format!("{pad}- {}", inline(item))]
                } else {
                    let mut lines = block(item, indent + 2);
                    lines[0] = format!("{pad}- {}", lines[0].trim_start());
                    lines
                }
            })
            .collect(),
        Node::Text(_) | Node::Number(_) => vec![format!("{pad}{}", inline(node))],
    }
}
