CREATE TABLE graphs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    root TEXT NOT NULL,
    language TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    meta_json TEXT NOT NULL,
    stats_json TEXT NOT NULL,
    top_json TEXT NOT NULL,
    warnings_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    graph_id INTEGER NOT NULL REFERENCES graphs(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE(graph_id, path)
);

CREATE TABLE nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    graph_id INTEGER NOT NULL REFERENCES graphs(id) ON DELETE CASCADE,
    stable_key TEXT NOT NULL,
    name TEXT NOT NULL,
    qname TEXT NOT NULL,
    kind TEXT NOT NULL,
    file TEXT NOT NULL,
    external INTEGER NOT NULL,
    fan_in INTEGER NOT NULL,
    fan_out INTEGER NOT NULL,
    call_in INTEGER NOT NULL,
    call_out INTEGER NOT NULL,
    page_rank REAL NOT NULL,
    hotspot REAL NOT NULL,
    dead_code INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE(graph_id, stable_key)
);

CREATE INDEX nodes_graph_fan ON nodes(graph_id, fan_in DESC, fan_out DESC);
CREATE INDEX nodes_graph_name ON nodes(graph_id, name);
CREATE INDEX nodes_graph_file ON nodes(graph_id, file);

CREATE TABLE edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    graph_id INTEGER NOT NULL REFERENCES graphs(id) ON DELETE CASCADE,
    source_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    source_key TEXT NOT NULL,
    target_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    sites INTEGER NOT NULL,
    external INTEGER NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX edges_graph_source ON edges(graph_id, source_id, kind);
CREATE INDEX edges_graph_target ON edges(graph_id, target_id, kind);
CREATE INDEX edges_graph_kind ON edges(graph_id, kind);

CREATE VIRTUAL TABLE nodes_fts USING fts5(
    name,
    qname,
    file,
    content='nodes',
    content_rowid='id',
    tokenize='trigram'
);

CREATE TRIGGER nodes_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts(rowid, name, qname, file)
    VALUES (new.id, new.name, new.qname, new.file);
END;

CREATE TRIGGER nodes_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, qname, file)
    VALUES ('delete', old.id, old.name, old.qname, old.file);
END;

CREATE TRIGGER nodes_au AFTER UPDATE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, qname, file)
    VALUES ('delete', old.id, old.name, old.qname, old.file);
    INSERT INTO nodes_fts(rowid, name, qname, file)
    VALUES (new.id, new.name, new.qname, new.file);
END;
