# code-graph

Call graphs and design metrics for a codebase, without building it.

code-graph answers the questions a reviewer actually asks: what does this change start
calling, who reaches the changed code, and what moved in the graph since the base commit.
It drives a language server (`rust-analyzer`, `tsserver`/`tsgo`, `clangd`, `pyright`) for
"what calls what" and tree-sitter for AST facts the server does not expose, computes
metrics once during analysis, and stores the result as JSON or a SQLite snapshot.

The snapshot layer is deliberately VCS-agnostic: `audit diff` takes two snapshots and a list
of changed paths, so the same tool serves a terminal review, a PR comment in CI, or an agent
asking for blast radius.

- `analyze` / `up` — build the graph and metrics for a project; `up` also serves the React
  explorer.
- `audit` — terminal overview, metric rankings, one symbol, cycles, dead code.
- `audit diff --view …` — what changed between two snapshots, as separate atomic tables.

## Quick start

```bash
cargo build --release
npm --prefix web install && npm --prefix web run build

./target/release/code-graph up . --exclude fixtures
# http://127.0.0.1:8787/ ; snapshots land in ./.code-graph/code-graph.db

./target/release/code-graph analyze fixtures/ts-project -o /tmp/ts-graph.json
./target/release/code-graph inspect /tmp/ts-graph.json
./target/release/code-graph audit .
```

## Install

Prebuilt binaries for Linux (static musl, x86_64 and aarch64) and macOS (arm64 and x86_64)
are published per tag — see [Releases](docs/releases.md). Each language needs its server on
`PATH` (TypeScript 5.9 for `typescript-language-server`, or `tsgo` for TypeScript 7); the
table is in [How analysis works](docs/analysis.md#language-servers).

## Documentation

| Document | Covers |
|---|---|
| [CLI](docs/cli.md) | commands, options, audit subcommands, snapshot diff views |
| [HTTP API and UI](docs/http-api.md) | `/api/v1` endpoints, explorer screens, refresh model |
| [Graph semantics](docs/graph-semantics.md) | nodes, edge kinds, result usage, stable identifiers |
| [Analysis](docs/analysis.md) | pipeline, language servers, tsgo/tsrs, readiness |
| [Metrics](docs/metrics.md) | definitions, formulas, per-language complexity rules |
| [Reading metric deltas (Russian)](docs/metrics-interpretation.ru.md) | practical thresholds and what to do when a metric grows |
| [Internals](docs/internals.md) | repository layout, run model, where metrics live, fixtures |
| [Releases](docs/releases.md) | tagging, targets, asset verification |
| [Limits](docs/limits.md) | what static LSP analysis cannot see |
