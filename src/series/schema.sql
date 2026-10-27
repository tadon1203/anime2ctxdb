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
    speaker_label TEXT
);
CREATE TABLE characters (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    reading TEXT,
    since_episode INTEGER NOT NULL,
    since_ms INTEGER NOT NULL
);
CREATE TABLE speaker_labels (
    label TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    character_id TEXT REFERENCES characters(id)
);
CREATE TABLE speaker_colors (
    color INTEGER PRIMARY KEY,
    character_id TEXT NOT NULL REFERENCES characters(id)
);
-- character_id NULL: no speaker. Then method is NULL too.
CREATE TABLE line_speakers (
    line_id TEXT NOT NULL REFERENCES lines(id),
    character_id TEXT REFERENCES characters(id),
    method TEXT
);
CREATE TABLE suggestions (
    id TEXT PRIMARY KEY,
    prompt_id INTEGER NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    blocked INTEGER NOT NULL,
    content TEXT NOT NULL
);
