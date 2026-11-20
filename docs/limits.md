# Limits

- Static, LSP-based analysis: dynamic dispatch, reflection, generated code, and calls hidden inside macros can be missed. Reference fallback only covers zero-incoming callables.
- The result-usage check is AST-based but conservative; macro arguments and unsupported grammars yield `unknown`.
- Dead-code detection is a heuristic: exported flags come from declaration text, and handlers registered at module scope are treated as used via module references.
- `--include-external` can explode the node count; the default records only distinct external callee counts.
- A single report covers one language/project; use separate JSON files for Rust backend and TS frontend.
- API mode bootstraps a ranked subset (default 200 nodes) and expands on demand; it never ships the whole graph. Search and path queries run server-side.
- The per-snapshot adjacency index is held in memory; it grows with edge count, while SQLite stays on disk.
- SQLite keeps every analyzed snapshot; snapshot pruning/retention is not implemented yet.
- `serve` is read-only. `up` starts one analysis; there is no endpoint to retrigger analysis on an already-running server. Status is polled, not pushed via SSE.
- TypeScript 7 is unsupported by `typescript-language-server` at the time of writing; install TypeScript 5.9.
- `clangd` and `pyright` readiness titles are best effort; their probes fall back to an empty-answer check within the budget.
