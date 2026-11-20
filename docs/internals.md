# Internals

## Repository layout

```text
src/
  main.rs              binary entry point, module list
  cli/
    args.rs            clap arguments
    inspect.rs         human-readable graph summary
    mod.rs             command dispatch + report writing
  analysis/
    options.rs         AnalyzeOptions
    summaries.rs       per-file aggregates
    mod.rs             LSP orchestration: discover -> symbols -> calls -> refs -> metrics
  graph/
    model.rs           serde node/edge schema (camelCase, consumed by the SPA)
    builder/
      mod.rs           builder state and public build API
      symbols.rs       document symbols, node creation, filtering
      targets.rs       call-target resolution, external nodes
      edges.rs         call/data edge aggregation, result usage
      hierarchy.rs     call hierarchy traversal
      callbacks.rs     callbacks passed as call arguments
      references.rs    constant references + missed-call fallback
  metrics/
    model.rs           stats and top-list schema
    mod.rs             metrics API
    compute.rs         degree/reachability/hotspot/dead-code computation
    scc.rs             Tarjan SCC + topological order
    centrality.rs      PageRank and betweenness
    top.rs             ranked tables
  lsp/
    protocol.rs        LSP types and retryable error codes
    transport.rs       stdio framing, reader/writer/dispatch tasks
    client.rs          request/notify API, initialization, shutdown
  languages/
    adapters.rs        server/extension/AST-node tables
    mod.rs             detection, grammar selection, language ids
    servers/           per-server readiness policy (rust-analyzer, tsserver, tsgo, ...)
  source/
    discovery.rs       file walking and tree-sitter parsing
    ast.rs             params, cyclomatic complexity, result usage
  store/
    mod.rs             SQLite snapshots, FTS5 search, node/edge payloads
  query/
    mod.rs             per-snapshot CSR cache, slices, shortest path, impact
    entry.rs           entry-point ranking, test filtering
    overview.rs        directory/file aggregation and bounded file slices
  api/
    mod.rs             axum routes, live analysis status, static SPA fallback
  progress.rs          phase/message events for CLI + HTTP job state
  report.rs            final JSON report contract
migrations/
  0001_init.sql        graph/nodes/edges/files schema + FTS triggers
web/
  src/
    App.tsx             screen composition only
    features/
      snapshot/         snapshot loading and static report fallback
      directories/      directory list screen
      files/            file list screen
      graph/            React Flow canvas, cards, layout, sidebar, entry panel
      nodes/             all-nodes table and context menu
    lib/api.ts          HTTP client
    shared/format.ts    formatting helpers
    types.ts            shared report/node/overview contracts
fixtures/              small projects with known graph properties
```

## Run model

Analysis runs on its own thread with a single-threaded Tokio runtime and blocks it until the
snapshot is written, so the HTTP server is not competing with the LSP client loop. `up` binds the
port first and reports progress through `/status`; the analysis itself is not resumable and there
is exactly one job per process.

## Where metrics live

- `analysis::metrics` computes the full `NodeMetrics` for every node in memory.
- `store` writes the complete node payload — including `nodes[].metrics` — to `nodes.payload_json`. Filterable metrics are duplicated into columns: `fan_in`, `fan_out`, `call_in`, `call_out`, `page_rank`, `hotspot`, `dead_code`.
- Per-snapshot `stats`, `top`, `warnings`, and `meta` are stored as JSON in the `graphs` row.
- `query` uses the stored node/edge payloads and builds an adjacency index for traversal; it does **not** recompute metrics. Bootstrap ordering uses the `fan_in`/`fan_out`/`page_rank` columns, and search ranking uses `fan_in`.
- If rows in SQLite are edited by hand, metrics become stale. Re-run `analyze`/`up` to write a fresh snapshot.

## Verification

```bash
./target/debug/code-graph analyze fixtures/ts-project -o /tmp/ts.json
./target/debug/code-graph inspect /tmp/ts.json
# 9 nodes, 6 call edges, 2 data edges, ignored results 16.7%

./target/debug/code-graph analyze fixtures/rust-project -o /tmp/rust.json
# 10 nodes, 7 call edges (one inferred: main -> scale through println!), 4 dead-code candidates

./target/debug/code-graph analyze fixtures/rust-cycle -o /tmp/cycle.json
# 1 cycle, SCC size 2 (mutual recursion a <-> b)

./target/debug/code-graph analyze fixtures/ts-arrows -o /tmp/arrows.json
# arrow-function const is promoted to a function node; new Box() targets Box.constructor
```

The TypeScript fixture checks both directions of the result flow: `ignoresResult -> mid` is `none`, every other call is `all`.

`cargo test --locked` covers the pure parts (identifiers, SCC, PageRank, progress parsing, audit
views, readiness resolution); the fixtures above cover the end-to-end pass.
