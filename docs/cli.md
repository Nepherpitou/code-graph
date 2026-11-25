# CLI

```text
code-graph analyze <dir> [options]
code-graph up [dir] [options]
code-graph inspect <graph.json>
code-graph audit [path] [options]
code-graph audit top [options]
code-graph audit focus <symbol> [options]
code-graph audit cycles [options]
code-graph audit dead-code [options]
code-graph audit search <text> [options]
code-graph audit metrics [options]
code-graph audit diff [options]
code-graph serve --db <database> [--bind 127.0.0.1:8787] [--web-dir web/dist]
```

| Option | Meaning |
|---|---|
| `-o, --out <file>` | Output JSON (default `graph.json`). |
| `--db <file>` | SQLite snapshot database (WAL + FTS5). `up` defaults to `<dir>/.code-graph/code-graph.db`; `analyze` persists only when this flag is given; `serve` defaults to `./.code-graph/code-graph.db`. |
| `--lang <id>` | `typescript`, `rust`, `cpp`, `python`, `auto`. |
| `--server-cmd <cmd>` | Override the server command; args are split on spaces. |
| `--include-external` | Keep library/node_modules callees as `external` nodes. |
| `--no-constants` | Omit constant/variable nodes and reference crawling. |
| `--no-ref-fallback` | Disable the references-based rescue for missed calls. |
| `--max-nodes <n>` | Hard node cap (default 20000); partial graphs are reported. |
| `--max-constants <n>` | Constant reference crawl cap (default 10000). |
| `--exclude <paths>` | Extra ignored paths, relative to `<dir>`; repeatable or comma-separated. |
| `-v, --verbose` | LSP protocol logging. |
| `--quiet` | Suppress progress output. |

`inspect` prints graph stats and the top lists that the SPA also renders.

## Audit CLI

`audit` is the terminal path for complexity and architecture review. It is read-only after a snapshot exists, defaults to human-readable text, and does not use smell thresholds or verdicts. It reuses the latest project snapshot; if none exists it analyzes first.

```bash
# Overview: counts, cycles, dead-code candidates, top hotspot scores
code-graph audit .

# Re-analyze explicitly
code-graph audit . --refresh

# Rank callables by an existing metric
code-graph audit top --order-by complexity --limit 10
code-graph audit top --order-by blast-radius --file src/domain

# One symbol: metrics, callers, callees, entry → exit route
code-graph audit focus createOrder

# Factual SCCs and existing dead-code candidates
code-graph audit cycles
code-graph audit dead-code --limit 10

# Callables whose direct calls largely overlap another symbol's
code-graph audit similar --limit 20
code-graph audit similar --file src/domain

# Search and metric glossary
code-graph audit search payment
code-graph audit metrics

# Compare two snapshots
code-graph audit diff --from 1 --to 12
code-graph audit diff --refresh
```

Common flags: `--root <path>` (for subcommands), `--db <file>`, `--snapshot <id|latest>` (all audit subcommands except `diff`), `--format text|json`. The overview can take the path directly (`audit .`); subcommands use `--root`. `audit diff` selects snapshots with `--from`/`--to` instead of `--snapshot`.

When an audit builds a snapshot (no snapshot exists, or `--refresh` is passed), it accepts the same analysis-control flags as `analyze`: `--lang`, `--exclude`, `--server-cmd`, `--include-external`, `--no-constants`, `--no-ref-fallback`, `--max-nodes`, `--max-constants`, `--verbose`, `--quiet`. They are ignored when an existing snapshot is read.

`audit similar` searches one snapshot rather than a diff: every comparable callable is a candidate, with no changed-symbol filter and no base snapshot. It uses the same fixed thresholds as `audit diff --view similar`: both sides call at least three symbols, at least three of them are the same node ids, and the common part covers at least half of the smaller call set; a direct call between the pair is skipped. Each symbol keeps one best partner, one row per pair, sorted by shared callees. `--file` narrows the pairs to either side, `--limit` caps the rows. `-v`/`--verbose` switches from the clipped table to one block per pair with full qualified names, file paths, and every shared call; the same flag enables LSP logging when `--refresh` builds a snapshot.

`audit top --order-by` accepts: `priority`, `hotspot`, `complexity`, `blast-radius`, `transitive-fan-out`, `fan-in`, `fan-out`, `call-fan-in`, `call-fan-out`, `data-fan-in`, `data-fan-out`, `ifc`, `betweenness`, `loc`, `sloc`, `params`. `blast-radius` and `transitive-fan-out` are the transitive fan-in and fan-out; the latter is the `reach` explained in [`metrics.md`](metrics.md#reach). `fan-in`/`fan-out` are total degrees; the `call-*`/`data-*` variants split them by edge kind. `ifc` and `priority` use call edges only. `priority` is an absolute refactoring score: complexity times transitive impact times call instability. `hotspot` remains a relative centrality triage score. `audit metrics` prints the glossary of every metric, including the rule that graph metrics ignore override, DI-wiring and test edges. `--min` is a numeric filter, not a smell threshold. `--file` and `--kind` narrow the rows.

`--format json` returns the same facts in a compact envelope for agents:

```json
{"kind":"audit.focus","snapshot":{...},"data":{"node":{...},"metrics":{...},"callers":[],"callees":[],"route":{...}}}
```

The audit deliberately has no `smells`, severity, or configurable thresholds yet. Cycles are SCCs; dead code is the existing candidate predicate; everything else is a raw metric ranking.

`priority` is snapshot-independent and uses SLOC (non-empty, non-comment lines) rather than the declaration span:

```text
priority = (cyclomatic + ln(1 + sloc))
         * (1 + log2(1 + transitiveFanIn))
         * (1 + callInstability)
```

Old snapshots that predate these fields still load; their `priority` is `0` until the project is re-analyzed.

### Snapshot diff

`audit diff` compares two persisted snapshots. It is VCS-agnostic: the command never invokes
Git, creates worktrees, or guesses commits. Select snapshots by id, `latest`, or `previous`:

```bash
# Latest two snapshots
code-graph audit diff

# Explicit pair
code-graph audit diff --from 1 --to 12

# Analyze the current tree, store a new snapshot, and compare it with the previous latest
code-graph audit diff --refresh
```

Without `--refresh`, `--from` defaults to `previous` and `--to` to `latest`. With `--refresh`,
the current latest snapshot is the baseline, the project is analyzed again, and the new snapshot
is the target. `--refresh` cannot be combined with `--to`.

Rows are matched by stable id first, then `(file, kind, qname)` for signature changes, then
unique `(kind, qname)` for moves. Added/removed symbols are reported separately. The default
report hides rows whose only change is a graph-wide metric such as hotspot drift; use
`--all-metrics` to include them.

`--order-by` uses the same metric names as `audit top` (`priority`, `complexity`, `sloc`, `loc`,
`fan-in`, `fan-out`, `transitive-fan-out`, ...), plus `name`. In `audit diff`, metric values are **deltas**: positive means the
value increased. Added rows sort by their new absolute value, removed rows by their old absolute
value.

Generic filters make commit/PR review possible without coupling the tool to Git:

```bash
# Paths are relative to the analysis root; a suffix match also supports subdirectory roots.
git diff --name-only base...head > /tmp/changed.txt
code-graph audit diff --from previous --to latest --changed-files /tmp/changed.txt

# Machine-readable output
code-graph audit diff --from 11 --to 12 --format json
```

The tool only reads the path list; producing it is the caller's responsibility. For two commits,
analyze each checkout/worktree into the same database and run `audit diff`; for a PR, use the
merge-base as the baseline. `--edges` adds call/data edge deltas, and `--exit-code` exits 1 when
the filtered diff contains any rows.

### Views

`--view <name>` prints one atomic table per call. Every view shares the snapshot selectors,
`--changed-files`, `--format`, `--limit` and `--ascending`; nothing here knows about Git or about
comment formatting, so callers compose whatever report they need.

| View | Answers |
|---|---|
| `symbols` (default) | which symbols changed and by how much |
| `summary` | graph shape: callables, call edges, cross-module calls, mean cyclomatic over all callables, over the touched ones and over the added ones, loc of analyzed files, cycles, dead-code candidates outside tests |
| `uses` | which existing symbols the changed code starts calling |
| `dependencies` | new call edges aggregated by module pair, with mutual pairs |
| `callers` | calls from unchanged files into the changed ones |
| `affected` | unchanged callables whose execution path reaches changed code (`changed`, `added`, `share`, `priority`, `direct`); sorted by `share`, then `priority` |
| `areas` | per-module added/removed/changed counts, mean cyclomatic per callable, loc delta |
| `filtered-edges` | how many added call edges survived, and why the rest were dropped (overrides of a base method, test files, DI wiring, interface/implementation duplicates, signature re-resolutions) |
| `cycles` | cycles added, removed, or unchanged, with members |
| `dead-code` | dead-code candidates that appeared in the target snapshot, with a DI/decorator flag; constants are skipped when either snapshot truncated the reference crawl (`constantsTruncated` in JSON) |
| `reach` | changed callables whose transitive fan-out moved (or that lost all callers or live in framework files): reach and fan-out before → after, plus the calls they added |
| `fan-in` | changed callables: caller count before → after, instability before → after, number of calling modules, and a `choke-point` flag when a symbol absorbed callers from several modules in one change |
| `similar` | changed callables whose direct calls largely overlap another symbol's: both sides call at least three symbols, at least three of them are the same node ids, and the common part covers at least half of the smaller call set; a direct call between the two, and pairs that already matched in the base snapshot, are skipped; one best match per changed symbol, with the shared calls listed |
| `silent-files` | changed files that carry no symbol rows (ports, DTOs, schemas, tests) |

`callers`, `affected`, `areas` and `silent-files` require `--changed-files`; the other views use
the whole snapshot pair.

Per-language conventions — test markers, framework suffixes, abstract markers, shared plumbing and
the reasons a file produces no symbol rows — live in `src/cli/audit_views/conventions.rs`; nothing
else in the views matches stack-specific paths. Example composition:

```bash
S="--db .code-graph/code-graph.db --changed-files /tmp/changed.txt --format json"
code-graph audit diff $S --view summary        > summary.json
code-graph audit diff $S --view uses --limit 10 > uses.json
code-graph audit diff $S --view affected --limit 10 > affected.json
```

Composing the views into a report is the caller's job. The renderer this repository uses in CI
(`.github/scripts/pr-report.py`) prints nothing and exits with code 3 when every view is empty,
so a workflow can delete a stale comment instead of publishing one that says "no changes".
