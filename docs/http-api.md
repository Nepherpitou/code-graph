# HTTP API and UI

`serve` exposes snapshot-scoped endpoints under `/api/v1`:

| Endpoint | Purpose |
|---|---|
| `GET /status` | Live analysis state: phase, message, timestamps, graph id, `canAnalyze`. |
| `POST /refresh` | Re-read the configured project from disk and store a new snapshot. Returns `409` while an analysis is running; only available with `up`. |
| `GET /graphs` | Snapshot list with meta/stats/top. |
| `GET /graphs/{id}` | One snapshot (`latest` is accepted). |
| `GET /graphs/{id}/overview?group=dir\|file` | Aggregated directory/file nodes and call edges. |
| `GET /graphs/{id}/file?path=&group=dir\|file&limit=500` | Nodes and internal edges for one directory/file slice. |
| `GET /graphs/{id}/entry-points?roots=8&depth=2&limit=180` | Outgoing-only graph for the heaviest entry points, plus the ranked root list. |
| `GET /graphs/{id}/nodes?limit=20000` | Full node payloads for the client-side node table. |
| `GET /graphs/{id}/bootstrap?limit=200` | Initial visible slice ranked by fan-in. |
| `GET /graphs/{id}/search?q=&kind=&limit=` | FTS5/trigram node search. |
| `GET /graphs/{id}/subgraph?roots=...&depth=1&limit=500` | Neighborhood expansion. |
| `GET /graphs/{id}/focus?node=&limit=180` | Direct neighbours, shortest entry→node→exit route, and two-hop context around a selected node. |
| `GET /graphs/{id}/path?from=&to=&edges=call\|all&maxDepth=12` | Shortest call path as node/edge payloads. |
| `GET /graphs/{id}/impact?node=&depth=2&direction=in\|out` | Caller/callee impact set. |

The graph store keeps normalized rows for filtering and full node/edge payloads for the UI. Adjacency is cached in memory per snapshot; BFS never runs in SQLite. A virtualized table holds all node payloads for search and direct navigation, so the canvas never tries to render every function at once.

## UI screens

1. **Entry points** (default) — the 8 heaviest roots, ranked by transitive fan-out (reachable callables). The API traverses outgoing calls two hops from those roots, merges shared callees, and caps the result at 180 nodes. Root cards carry a `root` chip; the sidebar shows the ranking and reach counts.
2. **Directories** — a list of aggregated directories with node/callable/constant counts, call volume, dead-code candidates, and cycle nodes. No graph canvas.
3. **Files** — a list of files inside the selected directory, with the same counts.
4. **Graph** — functions in the selected file, the entry-point graph, or the neighborhood of a node opened from the table. Click a card to replace the view with its focused slice: direct neighbours, the shortest execution route from the nearest entry point through the node to the nearest exit point, and two-hop context (up to 180 nodes, the same budget as the initial entry-point view). Click a node in that slice to keep drilling. **Top entries** returns to the overview. Each card shows the container name on one line and the member name on the next. Right-click opens actions: focus connected slice, show callers, show callees, open containing file, copy ID, copy qualified name.

The directory and file screens are plain lists; only the graph screens use React Flow.

## Snapshots and refresh

`up` stores snapshots inside the analyzed project at `<project>/.code-graph/code-graph.db`, so pointing it at another project cannot reuse that project's graph. The directory is hidden and excluded from source discovery. Pass `--db` to share a database explicitly.

`up` binds the HTTP port first. If the database already contains a snapshot, it loads the latest one and does not re-analyze; the header shows a **Refresh** button that calls `POST /refresh`, re-reads the project, stores a new snapshot, and reloads the UI. If the database is empty, `up` analyzes immediately. Progress is printed to the terminal and exposed through `/status`; the SPA polls it, shows the current phase/message, and loads the snapshot automatically when it becomes `ready`. `serve` only exposes existing snapshots and has no Refresh button.
