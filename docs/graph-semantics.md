# Graph semantics

Nodes are code primitives: functions, methods, constructors, and constants/variables. Class/struct/impl/interface nodes are containers only; their constructor becomes the callable node.

| Edge | Direction | Meaning |
|---|---|---|
| call, result ignored | caller → callee | A call whose return value is discarded (or unknown). |
| call, result used | caller ↔ callee | `resultUsed` is `all`/`some`; a second arrowhead points back at the caller. |
| data | function → constant | A function references a constant/variable. |
| inferred | caller → callee | Found from callback arguments or `textDocument/references`, not call hierarchy (dashed). |

Not every call edge is a dependency. An edge that only mirrors an override link between two same-named methods, an edge in either direction of a constructor (DI wiring), and an edge touching a test file are stored in the graph but skipped by the metrics: reach, fan-in/out, cycles, centrality and dead-code heuristics read dependency edges only. The audit views classify the same edges through `graph::conventions`, so the two never disagree.

Result usage is decided from the tree-sitter AST: the call is an `expression_statement` → discarded; `void f()` → discarded; the callee position must be inside the call's callee field (so calls nested in macro arguments stay `unknown` instead of producing a wrong answer).

Calls missed by call hierarchy are recovered with `textDocument/references` for callables that currently have **zero** incoming calls. This catches calls inside Rust macros and function-pointer registrations, at the cost of possible references-that-are-not-calls. Such edges are marked `inferred`.

A callable passed as a call argument is attributed to the enclosing named callable. For `function foo(arr) { arr.map(bar).filter(buzz); }`, `foo` gets inferred edges to `bar` and `buzz`. Anonymous callbacks are not nodes themselves; calls inside `arr.map((x) => bar(x))` are attributed to `foo`.

## Identifiers

Node identifiers are line-independent:

```text
<relative-path>#<kind>:<qname>[@<signature-hash>]
```

Callables include a 64-bit FNV-1a hash of their normalized parameter and return types, so overloads get distinct IDs. Constants and variables omit the hash. Edges are `call:<source-id>-><target-id>` or `data:<source-id>-><target-id>`. A `#2`, `#3` suffix is added only for otherwise identical IDs. SQLite row IDs and snapshot IDs are autoincrement and are not stable across commits; use the payload `id` values.
