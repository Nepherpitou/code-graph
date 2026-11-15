# code-graph

Call graph and design metrics for source code, built on **LSP** (language server protocol) and **tree-sitter**.

- **Backend — Rust** (`src/`): spawns a language server, reads document symbols, walks `callHierarchy`, resolves constant references, computes graph metrics, writes JSON and/or a SQLite snapshot, and serves graph slices over HTTP.
- **Frontend — TypeScript/React** (`web/`): call graph explorer with rounded card nodes, ELK layouts, filters, node metrics, PNG export, server search, and shortest-path/impact queries.

The backend does not parse language semantics itself. LSP answers "what calls what"; tree-sitter supplies AST facts that LSP does not expose (parameters, cyclomatic complexity, whether a call's result is consumed).

## Quick start

```bash
cargo build --release
npm --prefix web install
npm --prefix web run build

# One command: bind HTTP immediately, analyze in the background, stream progress.
./target/release/code-graph up . --exclude fixtures
# http://127.0.0.1:8787/  (SPA auto-detects the API and reloads when ready)
# Snapshots go to ./.code-graph/code-graph.db.

# Or analyze to JSON/snapshot without a server.
./target/release/code-graph analyze . --exclude fixtures \
  -o web/public/graph.json --db .code-graph/code-graph.db
./target/release/code-graph analyze fixtures/ts-project -o /tmp/ts-graph.json
./target/release/code-graph analyze web -o web/public/graph-web.json

# Serve an existing snapshot without re-analyzing.
./target/release/code-graph serve --db .code-graph/code-graph.db --web-dir web/dist
# For static JSON development: npm --prefix web run dev
```

On a served API root the SPA probes `/api/v1/status` and uses the API automatically, including live analysis progress; `?api=/api/v1&id=latest` pins it explicitly. Without an API it falls back to `graph.json`; you can also drag any graph JSON onto the page or pass `?graph=/path/to/graph.json`.

## Requirements

| Language | Server | Notes |
|---|---|---|
| Rust | `rust-analyzer` | Waits for analyzer quiescence before crawling. |
| TypeScript/JavaScript | `typescript-language-server --stdio` | Needs `typescript` 5.x (the 7.x package has no `tsserver.js`). If `tsgo` or `tsrs` exists in `node_modules/.bin` or `PATH`, it is preferred automatically; `--server-cmd` overrides the choice. |
| C/C++ | `clangd` | Best effort without `compile_commands.json`. |
| Python | `pyright-langserver --stdio` | Adapter is present but untested in this prototype. |

Tree-sitter grammars are compiled into the Rust binary. No runtime WASM is needed.

For TypeScript, install the server next to the project (or globally):

```bash
npm install --no-save typescript@5.9.3 typescript-language-server@4.4.0
```

### Native TypeScript 7 servers

TypeScript 5 `tsserver` remains the fallback, but the analyzer now auto-detects a native server in the project's `node_modules/.bin` or on `PATH`: `tsgo` first, then `tsrs`, then `typescript-language-server`. `--server-cmd` always overrides detection.

```bash
# Go implementation (official TypeScript 7): selected automatically when installed
npm install --no-save @typescript/native-preview
./target/release/code-graph analyze . -o /tmp/tsgo.json

# Rust implementation (tsrs, experimental): selected if tsgo is absent
npm install --no-save tsrs
./target/release/code-graph analyze . -o /tmp/tsrs.json

# Explicit override, including a binary outside the project
./target/release/code-graph analyze . \
  --server-cmd '/tmp/tsgo/node_modules/.bin/tsgo --lsp --stdio' -o /tmp/tsgo.json
```

Both speak the TypeScript 7 language-server protocol. The client omits absent optional `detail` fields because both servers reject `detail: null`.

On the NFL backend snapshot (883 files, 3984 TS5 nodes), the current numbers are:

| Server | Time | Project call graph |
|---|---:|---|
| `typescript-language-server` | 27.4 s | 3984 nodes, 4837 call edges |
| `tsgo` | 10.0 s | 4546 nodes, 6409 call edges |
| `tsrs` | 8.5 s | 4546 nodes, 6409 call edges |

`tsgo` and `tsrs` produce the same project nodes/edges and all fixtures tested here. With `--include-external`, `tsgo` is more complete (527 external nodes / 3366 external edges vs 501 / 3315 for tsrs). **Use `tsgo` as the production default for TypeScript 7; use `tsrs` as the opt-in fastest backend while validating its beta LSP.**

## CLI

```text
code-graph analyze <dir> [options]
code-graph up [dir] [options]
code-graph inspect <graph.json>
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
| `--max-constants <n>` | Constant reference crawl cap (default 500). |
| `--exclude <paths>` | Extra ignored paths, relative to `<dir>`; repeatable or comma-separated. |
| `-v, --verbose` | LSP protocol logging. |
| `--quiet` | Suppress progress output. |

`inspect` prints graph stats and the top lists that the SPA also renders.

### HTTP API

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

### UI screens

1. **Entry points** (default) — the 8 heaviest roots, ranked by transitive fan-out (reachable callables). The API traverses outgoing calls two hops from those roots, merges shared callees, and caps the result at 180 nodes. Root cards carry a `root` chip; the sidebar shows the ranking and reach counts.
2. **Directories** — a list of aggregated directories with node/callable/constant counts, call volume, dead-code candidates, and cycle nodes. No graph canvas.
3. **Files** — a list of files inside the selected directory, with the same counts.
4. **Graph** — functions in the selected file, the entry-point graph, or the neighborhood of a node opened from the table. Click a card to replace the view with its focused slice: direct neighbours, the shortest execution route from the nearest entry point through the node to the nearest exit point, and two-hop context (up to 180 nodes, the same budget as the initial entry-point view). Click a node in that slice to keep drilling. **Top entries** returns to the overview. Each card shows the container name on one line and the member name on the next. Right-click opens actions: focus connected slice, show callers, show callees, open containing file, copy ID, copy qualified name.

The directory and file screens are plain lists; only the graph screens use React Flow.

`up` stores snapshots inside the analyzed project at `<project>/.code-graph/code-graph.db`, so pointing it at another project cannot reuse that project's graph. The directory is hidden and excluded from source discovery. Pass `--db` to share a database explicitly.

`up` binds the HTTP port first. If the database already contains a snapshot, it loads the latest one and does not re-analyze; the header shows a **Refresh** button that calls `POST /refresh`, re-reads the project, stores a new snapshot, and reloads the UI. If the database is empty, `up` analyzes immediately. Progress is printed to the terminal and exposed through `/status`; the SPA polls it, shows the current phase/message, and loads the snapshot automatically when it becomes `ready`. `serve` only exposes existing snapshots and has no Refresh button.

## Graph semantics

Nodes are code primitives: functions, methods, constructors, and constants/variables. Class/struct/impl/interface nodes are containers only; their constructor becomes the callable node.

| Edge | Direction | Meaning |
|---|---|---|
| call, result ignored | caller → callee | A call whose return value is discarded (or unknown). |
| call, result used | caller ↔ callee | `resultUsed` is `all`/`some`; a second arrowhead points back at the caller. |
| data | function → constant | A function references a constant/variable. |
| inferred | caller → callee | Found from callback arguments or `textDocument/references`, not call hierarchy (dashed). |

Result usage is decided from the tree-sitter AST: the call is an `expression_statement` → discarded; `void f()` → discarded; the callee position must be inside the call's callee field (so calls nested in macro arguments stay `unknown` instead of producing a wrong answer).

Calls missed by call hierarchy are recovered with `textDocument/references` for callables that currently have **zero** incoming calls. This catches calls inside Rust macros and function-pointer registrations, at the cost of possible references-that-are-not-calls. Such edges are marked `inferred`.

A callable passed as a call argument is attributed to the enclosing named callable. For `function foo(arr) { arr.map(bar).filter(buzz); }`, `foo` gets inferred edges to `bar` and `buzz`. Anonymous callbacks are not nodes themselves; calls inside `arr.map((x) => bar(x))` are attributed to `foo`.

### Identifiers

Node identifiers are line-independent:

```text
<relative-path>#<kind>:<qname>[@<signature-hash>]
```

Callables include a 64-bit FNV-1a hash of their normalized parameter and return types, so overloads get distinct IDs. Constants and variables omit the hash. Edges are `call:<source-id>-><target-id>` or `data:<source-id>-><target-id>`. A `#2`, `#3` suffix is added only for otherwise identical IDs. SQLite row IDs and snapshot IDs are autoincrement and are not stable across commits; use the payload `id` values.

## Metrics

Metrics are computed **once during analysis**, in memory, before the snapshot is written. They are then persisted in SQLite (see [Where metrics live](#where-metrics-live)) and served from there. `serve` never recomputes them.

### Node metrics

Distinct degrees use project nodes; `--include-external` adds external nodes but external edges are excluded from graph metrics.

| Metric | Meaning / rule |
|---|---|
| `fanIn` | Distinct project nodes that call or reference this node. Blast radius for functions; referencing functions for constants. |
| `fanOut` | Distinct project nodes this node calls or references. Includes data edges to constants. |
| `callIn` / `callOut` | Call/reference **sites**, counting multiplicity. Ten calls in a loop contribute 10; fan-in/out still count the callee once. |
| `transitiveFanIn` | Number of callables that can reach this node through call edges, excluding itself. BFS over reversed calls; if there are more than 3000 callables, it falls back to direct `fanIn`. |
| `transitiveFanOut` | Number of callables reachable from this node through call edges, excluding itself. Data edges are not traversed. |
| `scc` / `sccSize` / `inCycle` | Tarjan component id, component size, and `sccSize > 1`. A component larger than one node is a call cycle. |
| `depth` / `height` | Position in the SCC condensation DAG: `depth` is 0 for a source component and grows along the longest path; `height` is 0 for a sink and measures the longest path to it. Cycles get one finite component value. |
| `pageRank` | PageRank over the combined call + data graph, damping 0.85, dangling mass redistributed, until 1e-9 or 100 iterations. Punia et al.'s CGBR analogue. |
| `betweenness` | Brandes betweenness over callable call edges only. Skipped when `callables × call_edges > 6,000,000`; then it stays 0. |
| `instability` | `fanOut / (fanIn + fanOut)`, Martin's instability analogy applied to functions. 0 = stable/depended on, 1 = unstable/depends on others. |
| `resultIgnoredRatio` | Ignored call sites / call sites with known result usage. Unknown sites (macros, unsupported syntax) are excluded. |
| `consumesResults` / `providesResults` | Number of call sites whose result is used where this node is the caller / callee. |
| `externalFanOut` | Distinct external/library callees recorded for this node. |
| `constantsUsed` | Distinct constant/variable targets referenced through data edges. |
| `moduleRefs` | References from module scope — outside any callable. Used by dead-code detection for constants and callback-style registrations. |
| `cyclomatic` | `1 + decision points` from tree-sitter. Nested functions are excluded. Language-specific decision nodes are listed in [`docs/metrics.md`](docs/metrics.md). |
| `params` | Explicit parameters; `self`/`this` receivers are excluded. |
| `loc` | Lines spanned by the declaration's LSP range. |
| `ifc` | Information-flow complexity: `loc + (fanIn × fanOut)²`. |
| `hotspot` | Triage score over callables, min-max normalized per snapshot: `0.35·transitiveFanIn + 0.20·fanIn + 0.15·cyclomatic + 0.15·loc + 0.15·fanOut`. Heuristic, not a scientific metric. |
| `deadCodeCandidate` | `callIn == 0 && moduleRefs == 0 && !exported && !entryLike && !name.startsWith('_') && kind != constructor && !inCycle`. Candidate, not proof. |

### Graph-level metrics

| Metric | Meaning |
|---|---|
| `nodes` / `functions` / `constants` | Project nodes, callables (function, method, constructor), and data nodes. |
| `edges` / `dataEdges` | Non-external call edges / data edges. |
| `externalEdges` | Sum of distinct external callees across callables. |
| `density` | `call_edges / (callables × (callables − 1))`. |
| `avgFanIn` / `avgFanOut` | Mean distinct fan-in/out over callables. |
| `maxFanIn` / `maxFanOut` | Maximum distinct fan-in/out over callables. |
| `cycles` / `nodesInCycles` / `largestCycle` | SCCs with more than one callable, nodes in them, and the largest such SCC. |
| `deadCode` | Number of nodes with `deadCodeCandidate`. |
| `isolated` | Callables with `fanIn + fanOut == 0`. |
| `maxDepth` / `maxHeight` | Deepest condensation level and tallest dependency chain among callables. |
| `crossFileEdges` / `crossFileRatio` | Call edges whose endpoints are in different files, and their share of all call edges. |
| `resultIgnoredRatio` | Global ignored sites / all sites with known result usage. |

`top` stores 10-entry leaderboards for fan-in, fan-out, PageRank, betweenness, hotspot, cyclomatic complexity, IFC, and dead-code candidates sorted by LOC.

### Where metrics live

- `analysis::metrics` computes the full `NodeMetrics` for every node in memory.
- `store` writes the complete node payload — including `nodes[].metrics` — to `nodes.payload_json`. Filterable metrics are duplicated into columns: `fan_in`, `fan_out`, `call_in`, `call_out`, `page_rank`, `hotspot`, `dead_code`.
- Per-snapshot `stats`, `top`, `warnings`, and `meta` are stored as JSON in the `graphs` row.
- `query` uses the stored node/edge payloads and builds an adjacency index for traversal; it does **not** recompute metrics. Bootstrap ordering uses the `fan_in`/`fan_out`/`page_rank` columns, and search ranking uses `fan_in`.
- If rows in SQLite are edited by hand, metrics become stale. Re-run `analyze`/`up` to write a fresh snapshot.

Definitions, per-language complexity rules, and researched-but-unimplemented metrics are in [`docs/metrics.md`](docs/metrics.md). Main sources: Punia et al. on call-graph metrics (fan-in/out, CGBR, IFC), Robert Martin's instability metric, McCabe cyclomatic complexity, Sonar's cognitive complexity, and Chidamber–Kemerer LCOM.

### Практическая интерпретация метрик: что значит их рост

Сразу дисклеймер: **ни одна метрика сама по себе не говорит «пиздец»**. Метрики — это датчики, а не приговор. Смотреть надо на дельту в одном и том же проекте, на ранг/перцентиль и на сочетания. Плюс после коммита нужно переанализировать граф — метрики не пересчитываются автоматически.

Условная шкала:

- 🟢 — информационно, действий не требует;
- 🟡 — посмотреть diff и добавить тесты;
- 🟠 — design review, риск для сопровождения;
- 🔴 — архитектурный красный флаг, мержить только с осознанным решением.

#### Структурные метрики и blast radius

| Метрика | Что значит на практике | Если выросла после коммита |
|---|---|---|
| `fanIn` | Сколько разных функций/констант зависят от узла. | 🟡 Если функция не задумывалась как публичный хаб — новый источник связности. 🟠 если одновременно выросли `cyclomatic`/`ifc`. |
| `fanOut` | Сколько разных функций/констант узел вызывает. | 🟡 Функция начала знать слишком много. Для функции >10–15 зависимостей — уже запах. 🟠 если высокий и `fanIn`. |
| `callIn` / `callOut` | Количество **мест** вызова, с кратностью. | 🟢/🟡 Само по себе не архитектура: десять вызовов в цикле не добавят десять `fanOut`. Рост `callOut` без роста `fanOut` — повторные вызовы, возможная-performance тема. |
| `transitiveFanIn` | Сколько функций **транзитивно** могут до тебя добраться. | 🟠 Это blast radius. Если удвоился — изменение в узле задевает гораздо больше кода. Особенно плохо у «обычной» функции, а не у entrypoint/утилиты. |
| `transitiveFanOut` | Сколько функций транзитивно достижимо из узла. | 🟡/🟠 Поверхность зависимостей: сложнее тестировать, больше причин для каскадных изменений. |
| `sccSize` / `inCycle` | Размер SCC и факт цикла. | 🟠 Новый цикл — почти всегда дизайн-ревью. 🔴 если SCC большой или межмодульный. Взаимная рекурсия может быть легитимной, но случайный цикл — нет. |
| `depth` | Насколько глубоко SCC находится от источников конденсации. | 🟡 Длиннее цепочка вызовов. 🟠 если резко вырос `maxDepth` или функция стала труднодостижимой. |
| `height` | Длина самого длинного пути до стока. | 🟠 Глубина зависимостей: сложнее тестировать, дольше каскад. 🔴 при большом скачке вместе с циклами. |
| `pageRank` | Важность в графе вызовов + использования констант. | 🟢/🟡 Сама по себе нейтральна. Если узел неожиданно вошёл в топ-10 — посмотреть, почему он стал центральным. |
| `betweenness` | Насколько часто узел лежит на кратчайших путях. | 🟠 Broker/single point of failure. Резкий рост = через него теперь идёт много маршрутов. Если метрика обнулилась из-за лимита `6M` операций — это не улучшение. |

#### Связность, данные, сложность

| Метрика | Что значит | Рост |
|---|---|---|
| `instability` | `fanOut / (fanIn + fanOut)`. 0 — на тебе все завязаны, 1 — ты завязан на всех. | 🟡 Не «плохо/хорошо», а сдвиг архитектуры. Рост к 1 — узел стал более зависимым; падение — более стабильным. Смотреть вместе с `fanIn/out`. |
| `resultIgnoredRatio` | Доля вызовов с известным результатом, где результат выброшен. | 🟡 Рост = больше side-effect-стиля. Для команд/обработчиков нормально; для функций, которые должны возвращать данные, — запах. |
| `consumesResults` | Сколько раз результат **этого** узла был использован. | 🟢 Нейтрально; рост = больше реального dataflow. |
| `providesResults` | Сколько раз результат **вызовов из этого узла** использован. | 🟢/🟡 Рост = сильнее контракт; не проблема сама по себе. |
| `externalFanOut` | Разные внешние/library-каллеи. | 🟡 Больше внешней зависимости: тесты, портируемость, supply chain. 🟠 если это core/domain-код. |
| `constantsUsed` | Сколько разных констант/переменных читает функция. | 🟡 Data coupling. Много глобальных констант — скрытая связанность и сложность тестов. |
| `moduleRefs` | Ссылки из module scope, вне функций. | 🟢/🟡 Обычно регистрация обработчиков/конфигов. Смотри, если это неожиданный wiring. |
| `cyclomatic` | `1 + число ветвлений`. | 🟡 Рост = больше путей и тестов. Правило большого пальца: >10 — присмотреться, >20 — 🟠, >30 — 🔴. Но зависит от команды и языка. |
| `params` | Число явных параметров. | 🟡 >4–5 — запах. Рост публичного API — 🟠: ломать/тестировать дороже. |
| `loc` | Размер функции в строках. | 🟢 Сам по себе почти ничего не значит. 🟡 если растёт вместе с `cyclomatic`, а не выделяется в helper. |
| `ifc` | `loc + (fanIn × fanOut)²`. | 🟠/🔴 Очень нелинейная. Даже небольшой рост произведения fan-in × fan-out может взорвать значение. Смотреть на порядок величины, не на проценты. |
| `hotspot` | Эвристический триаж-бленд: blast radius + fan-in/out + сложность + LOC. | 🟠 Если узел вошёл в топ — приоритет ревью/тестов. Метрика относительная, не абсолютная. |
| `deadCodeCandidate` | Кандидат в мёртвый код по эвристике. | 🟡 Новый кандидат после коммита — проверить. Могут быть false positives: reflection, DI, обработчики, публичный API без внутренних вызовов. Это не «код не работает», а «нет видимых ссылок». |

#### Графовые агрегаты

| Метрика | Что значит | Рост |
|---|---|---|
| `nodes` / `functions` / `constants` | Размер графа. | 🟢 Ожидаемо при росте проекта. Все остальные метрики надо нормировать на это. |
| `edges` / `dataEdges` | Объём связей. | 🟡 Если рёбра растут быстрее узлов — граф становится плотнее. |
| `externalEdges` | Суммарная внешняя поверхность. | 🟡 Рост внешних зависимостей. |
| `density` | `E / (N × (N−1))`. | 🟠 Для большой системы рост плотности — движение к монолиту. На маленьком графе высокое значение нормально. |
| `avgFanIn` / `avgFanOut` | Средние степени. | 🟡 Показывают общий тренд связанности; для действий полезнее `max*`. |
| `maxFanIn` | Самый нагруженный узел. | 🟠 Новый hub. Особенно если это не инфраструктурная функция. |
| `maxFanOut` | Самый «всезнающий» узел. | 🟠 Новый god-function / координатор, который надо разрезать. |
| `cycles` | Число SCC с циклом. | 🟠 Любой новый цикл — повод на ревью. 🔴 если их несколько или они межмодульные. |
| `nodesInCycles` / `largestCycle` | Масштаб цикличности. | 🔴 Большой SCC — самая неприятная форма: тестирование, слоистость, дебаг. |
| `deadCode` | Количество кандидатов. | 🟡 Рост мёртвого кода. Сначала отфильтровать false positives. |
| `isolated` | Callable без входящих и исходящих. | 🟢/🟡 Это entrypoint, callback или мёртвый код. Рост — проверить, не потеряли ли wiring. |
| `maxDepth` / `maxHeight` | Самые длинные цепочки в condensation DAG. | 🟠 Рост = глубже зависимости, дороже тесты и каскадные изменения. |
| `crossFileEdges` / `crossFileRatio` | Вызовы между файлами. | 🟡 Не всегда плохо: интеграция растёт. 🟠 если растёт через границы модулей/слоёв; слишком низкий ratio — тоже подозрительно (силосы). |
| `resultIgnoredRatio` (global) | Доля выброшенных результатов по графу. | 🟡 Тренд к side-effect-архитектуре. |

#### Top-списки

`fanIn`, `fanOut`, `pageRank`, `betweenness`, `hotspot`, `complexity`, `ifc`, `deadCode` — это лидерборды по 10 позиций. Смотреть не на абсолютное значение, а на **появление нового имени в топе** или **переход узла из середины в топ**. Новый узел в топе `hotspot`/`ifc` после коммита — самый практичный сигнал «здесь надо добавить тесты и подумать о декомпозиции».

#### Как читать дельту после коммита

1. Сначала `cycles`, `largestCycle`, `maxDepth`, `maxHeight` — это структурные красные флаги.
2. Потом `hotspot`, `ifc`, `transitiveFanIn`, `maxFanIn` — blast radius и риск.
3. Потом `fanOut`, `cyclomatic`, `params`, `externalFanOut` — локальная сложность/связанность.
4. `deadCode`, `isolated`, `resultIgnoredRatio` — гигиена и false-positive проверка.
5. `pageRank`, `betweenness`, `nodes`, `loc`, `density` — контекст; без первых пунктов они мало что решают.

И главное: метрики надо сравнивать при **одинаковом языке, сервере и версии анализатора**. Смена LSP-сервера или парсера может дать ложную дельту.

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
      nodes/            all-nodes table and context menu
    lib/api.ts          HTTP client
    shared/format.ts    formatting helpers
    types.ts            shared report/node/overview contracts
fixtures/              small projects with known graph properties
```

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

## Limits

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
