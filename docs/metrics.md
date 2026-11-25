# Metrics reference

All metrics are computed from one graph: project nodes are functions, methods, constructors, and constants/variables; call edges point caller → callee; data edges point function → referenced constant. External/library callees are counted but excluded from graph metrics unless `--include-external` is set.

Graph metrics count **dependency** edges only. Two same-named methods linked because one overrides the other, calls in either direction of a constructor (DI wiring), and calls to or from test files are stored as edges but do not contribute to `fanIn`/`fanOut`, reachability, cycles, centrality or priority. The site counters `callIn`/`callOut` and `resultIgnoredRatio` still count every call site, so a method that only tests call keeps a non-zero `callIn`.

The report carries every node metric in `nodes[].metrics`, graph aggregates in `stats`, and leaderboards in `top`. Where they are computed and persisted is described in [`internals.md`](internals.md#where-metrics-live); practical guidance on reading a delta after a commit is in [`metrics-interpretation.ru.md`](metrics-interpretation.ru.md).

## Structural metrics (base)

| Metric | Formula / rule | Notes |
|---|---|---|
| `fanIn` | distinct project functions that call or reference the node | Blast radius. For constants, the number of referencing functions. |
| `fanOut` | distinct project functions/constants the node calls or references | Constants count as data dependencies, matching the graph's edges. |
| `callIn` / `callOut` | call/reference **sites**, counting multiplicity | A loop calling `f()` ten times adds 10. |
| `transitiveFanIn` | BFS over reversed call edges, excluding self | "How many functions can reach this one." Capped at 3000 callables for cost; beyond that it equals direct fan-in. |
| `transitiveFanOut` | BFS over call edges, excluding self | Reachable surface, called `reach` elsewhere in the tool; see [Reach](#reach). Data edges are excluded. |
| `depth` | 0 for a source SCC; otherwise the longest edge distance from a source SCC | Computed on the SCC condensation DAG, so cycles get a finite value. |
| `height` | longest path from this SCC to a sink | Same condensation DAG. |
| `scc`, `sccSize`, `inCycle` | Tarjan strongly connected components | A component larger than one node is a call cycle. |
| `density` (graph) | `E / (N * (N - 1))` over callables | Directed edge density. |
| `crossFileRatio` (graph) | cross-file call edges / all call edges | A quick modularity smell: a very high ratio is normal in small projects, but a very low one suggests hidden coupling or dead modules. |

Structural fan-in/out are the classic call-graph metrics used for defect prediction (Aivosto; Punia et al.). Multiplicity is kept separate from distinct degree so that a single hot call site does not hide broad coupling.

## Reach

`reach` is the transitive fan-out of one symbol: the number of distinct project callables it can get to through dependency call edges in one or more steps, not counting itself. It is the mirror of blast radius (`transitiveFanIn`); both run the same BFS in opposite directions over the same edge set.

What counts:

- call edges only — a function that reads a constant has data coupling but no reach;
- excluded edges do not count: override links between same-named methods, DI constructor wiring, and any edge touching a test file (see the dependency rule at the top);
- external/library callees are not nodes unless `--include-external`, so they add nothing;
- computed exactly below 3000 callables; above that it falls back to direct fan-out.

A leaf has reach 0. A route handler usually has reach in the tens; a bootstrap or an "everything" coordinator can reach most of the graph.

Where the number appears:

- `audit top --order-by transitive-fan-out` ranks current values (`transitive-out` in the text header);
- the overview graph ranks entry points by reach (next section);
- `audit diff --view reach` reports how the reach of changed callables moved, together with the calls the change added.

Read it as surface, not risk: reach grows whenever a function starts calling a widely used helper, which is often the intended direction. The useful signal is the delta inside one change plus the new callees behind it. `audit metrics` prints the same definition in the CLI glossary.

## Entry-point ranking

The overview graph answers "where does the heaviest code start?" An entry point is a project callable (not a constructor) with:

- zero incoming call sites (`callIn == 0`);
- no test/spec attribution by path or member name;
- at least one reachable callable.

Roots are sorted by **`transitiveFanOut`**: the number of distinct callables reachable through outgoing call edges. This is the natural "subtree size" metric and avoids the entry-point bias of fan-in/PageRank, which are near zero for roots. Ties use `height`, then direct `fanOut`, then qualified name.

The graph endpoint traverses **outgoing call edges only**. Using the bidirectional neighborhood traversal here would pull in every caller of a shared callee and rebuild almost the entire graph. Defaults are 8 roots, 2 hops, and a 180-node cap: on a 3984-node backend snapshot this yields 158 nodes and 219 edges. Three hops explode to roughly 300 nodes; two hops is the useful entry → service → repository view.

`transitiveFanOut` is computed exactly below 3000 callables; above that limit it falls back to direct fan-out. Large snapshots therefore rank roots by breadth, not exact reach.

## Centrality

| Metric | Definition | Source / caveat |
|---|---|---|
| `pageRank` | PageRank over the combined call + constant-use graph, damping 0.85, dangling nodes redistributed, 100 iterations / 1e-9 tolerance | Punia et al. call this **CGBR** (call-graph based ranking). Combined graph means heavily used constants are visible too; pure call-graph PageRank would rank them 0. |
| `betweenness` | Brandes' algorithm, directed and unweighted, over call edges | "How often a function lies on shortest call paths." O(V·E); skipped above 6M operations, leaving zeros. |

Centrality answers a different question than degree: a small utility can have huge fan-in (high PageRank) while a coordinator with modest degree sits on every path between subsystems (high betweenness).

## Coupling and stability

| Metric | Formula | Interpretation |
|---|---|---|
| `instability` | `fanOut / (fanIn + fanOut)` | Robert Martin's package metric applied to functions: 0 = everyone depends on me, I depend on nobody; 1 = I depend on many, nobody depends on me. High fan-in **and** high fan-out marks a node nobody wants to change. |
| `externalFanOut` | distinct external callees | Library/OS surface used by the function. |
| `constantsUsed` | distinct constant/variable data edges | Data coupling inside the project. |
| `moduleRefs` | references from module scope | Constant usage or callable registration outside any function; excluded from call edges but relevant to dead-code decisions. |
| `resultIgnoredRatio` | ignored call sites / call sites with known usage | Data-flow smell: a function that calls many things but discards every result is side-effect-driven. Unknown sites (macros, unsupported syntax) are excluded. |

The `instability` metric is normally defined on packages/classes (`Ce/(Ca+Ce)`); applying it to functions is a deliberate prototype-level analogy. The coupling.dev and ast-metrics write-ups give the original definition and the Stable Dependencies Principle.

## Complexity and size

| Metric | Rule |
|---|---|
| `cyclomatic` | `1 + decision points`, counted from the tree-sitter AST; nested functions are excluded. |
| `params` | Explicit parameters; receivers (`self`, `this`) are excluded. |
| `loc` | Lines spanned by the declaration (LSP range). |
| `ifc` | `LOC + (fanIn × fanOut)²` — information flow complexity from Punia et al. |
| `hotspot` | Decision-first triage score: `raw = (cyclomatic−1) · (1+ln(1+transitiveFanIn)) · (1+ln(1+loc)) · (1+ln(1+fanOut))`, stored as `raw / max(raw)` so the value stays in 0..1. A function with no decisions scores zero regardless of fan-in; logarithmic factors compress heavy tails without thresholds. Only the final score is normalized, not each metric. Not a scientific metric. |

Cyclomatic decision nodes per language:

- **TypeScript/JS**: `if_statement`, `for_statement`, `for_in_statement`, `while_statement`, `do_statement`, `switch_case`, `catch_clause`, `conditional_expression`; `&&`, `||`, `??` binary operators.
- **Rust**: `if_expression`, `while_expression`, `for_expression`, `loop_expression`, `try_expression` (`?`); `&&`, `||`; a `match_expression` contributes `arms - 1`.
- **C/C++**: `if_statement`, `for_statement`, `while_statement`, `do_statement`, `case_statement`, `catch_clause`, `conditional_expression`; `&&`, `||`.
- **Python**: `if_statement`, `for_statement`, `while_statement`, `except_clause`, `conditional_expression`, `case_pattern`. Boolean operators are not counted yet because Python uses `boolean_operator`, not `binary_expression`.

This is McCabe's metric, not cognitive complexity: nesting is ignored, so a deeply nested but linear function can score lower than a flat switch. Sonar's cognitive complexity is the better readability metric, and the AST already exposes the nesting needed to compute it.

## Quality signals

- **Dead-code candidate**: `callIn == 0`, `moduleRefs == 0`, not exported, not entry-like (`main`, tests, framework hooks), name does not start with `_`, not a constructor, and not in a cycle. This catches unused private helpers. It can produce false positives for reflection, generated wiring, and handlers registered through patterns LSP cannot see; treat it as a candidate list, not proof. Constants and variables are only judged when their references were crawled: if `--max-constants` cut the crawl, the snapshot records the truncation and data-kind nodes are left out of the candidate list. Nodes in framework files — `*.controller.ts`, `*.module.ts`, `*.provider.ts`, `*.guard.ts`, `*.interceptor.ts` — are never candidates: decorators wire them without call edges, so a missing caller there means a route or a hook, not dead code.
- **Cycles**: SCCs larger than one node. Cycles are not automatically bad (mutual recursion, protocol state machines), but they break layering and complicate testing.
- **Ignored results**: see `resultIgnoredRatio`. A function with a high ratio is a candidate for returning `void` and documenting side effects.
- **Isolated nodes**: no incoming and no outgoing edges; usually entry points, callbacks, or dead code.
- **Depth/height**: long chains in a condensation DAG are testing and debugging cost; a high `height` means many transitive dependencies before a leaf result.

## Researched but not implemented

| Candidate | Why it is useful | Why it is absent / what it needs |
|---|---|---|
| Cognitive complexity (Sonar) | Penalizes nesting and breaks from McCabe's mathematical model; correlates better with readability | Needs nesting-aware AST walk, recursion-cycle increments, and per-language rule sets. Tree-sitter exposes all of it; this is the top follow-up. |
| LCOM / LCOM4 (Chidamber–Kemerer) | Detects classes whose methods share no fields (low cohesion) | Needs class/field-level AST access and method-to-field usage; the current graph intentionally stops at function level. |
| CBO / RFC | Class coupling and response-set size | LSP references can approximate accessor/method dependencies, but class-level aggregation and inheritance semantics are missing. |
| Change/logical coupling | Files that change together even without static dependencies | Requires VCS history (git log), outside a static LSP pass. |
| Test coverage / mutation score | Direct evidence of untested blast radius | Requires running the test suite and collecting coverage. |
| Halstead volume/effort | Token-level operator/operand complexity | Requires a token classifier per language; cyclomatic + LOC + IFC are already available. |
| Type-hierarchy metrics (DIT/NOC) | Inheritance depth and breadth | LSP has `typeHierarchy`; it is not wired into the graph yet. |
| Semantic clone detection | Duplication across functions | Requires embeddings or AST fingerprints; not an LSP feature. |

## Sources

- Punia, Kumar, Sharma. *Evaluation the Quality of Software Design by Call Graph based Metrics*, GJCST 2014 — LOC, fan-in/out, CGBR (PageRank-style), IFC. <https://globaljournals.org/GJCST_Volume14/9-Evaluation-the-Quality-of-Software-Design-by-Call.pdf>
- Martin's instability / afferent-efferent coupling: <https://ast-metrics.dev/metrics/coupling/> and <https://coupling.dev/posts/related-topics/afferent-and-efferent-coupling/>
- McCabe cyclomatic complexity: <https://en.wikipedia.org/wiki/Cyclomatic_complexity>
- SonarSource, *Cognitive Complexity*: <https://www.sonarsource.com/docs/CognitiveComplexity.pdf>
- Chidamber–Kemerer metrics suite, including LCOM: <https://www.cs.uah.edu/~letzkorn/joop.pdf>
- Aivosto, structural fan-in/fan-out: <https://www.aivosto.com/project/help/pm-sf.html>
- PageRank: <https://en.wikipedia.org/wiki/PageRank>; betweenness (Brandes): <https://en.wikipedia.org/wiki/Betweenness_centrality>
