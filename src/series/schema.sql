CREATE TABLE sources (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    episode INTEGER,
    language TEXT
);
CREATE TABLE lines (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(id),
    episode INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    text TEXT NOT NULL,
    color INTEGER,
    offscreen INTEGER NOT NULL,
    speaker_label TEXT,
    prompt_text TEXT NOT NULL
);
-- An entity is its name. The columns that hold an entity name (entity_id, place_id) can name an
-- entity that no answer introduces any more, so they have no REFERENCES.
CREATE TABLE entities (
    name TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    reading TEXT,
    since_episode INTEGER NOT NULL,
    since_ms INTEGER NOT NULL
);
CREATE TABLE facts (
    id TEXT PRIMARY KEY,
    text TEXT NOT NULL,
    since_episode INTEGER NOT NULL,
    since_ms INTEGER NOT NULL
);
CREATE TABLE fact_entities (
    fact_id TEXT NOT NULL REFERENCES facts(id),
    entity_id TEXT NOT NULL
);
CREATE TABLE fact_evidence (
    fact_id TEXT NOT NULL REFERENCES facts(id),
    line_id TEXT NOT NULL REFERENCES lines(id)
);
CREATE TABLE scenes (
    id TEXT PRIMARY KEY,
    episode INTEGER NOT NULL,
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    place_id TEXT,
    summary TEXT NOT NULL
);
CREATE TABLE speaker_labels (
    label TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    entity_id TEXT
);
CREATE TABLE speaker_colors (
    color INTEGER PRIMARY KEY,
    entity_id TEXT NOT NULL
);
-- entity_id NULL: no speaker. Then method is NULL too.
CREATE TABLE line_speakers (
    line_id TEXT NOT NULL REFERENCES lines(id),
    entity_id TEXT,
    method TEXT
);
