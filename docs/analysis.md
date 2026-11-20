# How analysis works

The analyzer never parses language semantics itself. It drives a language server for
"what calls what" and uses tree-sitter for AST facts the server does not expose
(parameters, cyclomatic complexity, whether a call's result is consumed).

Pipeline, per project:

1. discover source files and parse them with tree-sitter;
2. start the language server, `initialize`, send `textDocument/didOpen` for the files;
3. ask for `textDocument/documentSymbol` per file → the node universe;
4. wait until the server can answer about the project (see [readiness](#readiness));
5. walk `callHierarchy/outgoingCalls` for each callable → call edges;
6. crawl `textDocument/references` for constants → data edges, and for callables with no
   incoming calls → `inferred` edges;
7. compute metrics once, in memory, then write JSON and/or a SQLite snapshot.

## Language servers

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

## Native TypeScript 7 servers

TypeScript 5 `tsserver` remains the fallback, but the analyzer auto-detects a native server in the project's `node_modules/.bin` or on `PATH`: `tsgo` first, then `tsrs`, then `typescript-language-server`. `--server-cmd` always overrides detection.

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

## Readiness

Starting a server is not the same as the project being loaded. `textDocument/references`
on an unindexed workspace returns a short list without an error, so edges silently
disappear instead of failing loudly. Each server therefore gets a policy in
`src/languages/servers/`:

- **rust-analyzer** — waits for the `priming`/`indexing` progress notifications to settle,
  then probes `callHierarchy` until one sample answers; `flycheck` is ignored because it
  does not affect symbol resolution.
- **tsserver** — waits until the project is quiet, then probes call hierarchy.
- **tsgo** — no background index; it builds what a request needs, and document symbols are
  already collected, so it reports ready immediately.
- **clangd**, **pyright** — wait for their progress titles when present, otherwise probe
  call hierarchy and `workspace/symbol` within a budget.
- **generic** — probe only.

Every probe request is bounded by the remaining budget (60 s total), so a server that
holds a request cannot stretch the analysis past it.

## LSP surface used

- `initialize` capabilities: `documentSymbol`, `callHierarchy`, `references`, `workspace/symbol`.
- `textDocument/documentSymbol` → node universe (functions, methods, constructors, constants, class/impl containers).
- `textDocument/prepareCallHierarchy` + `callHierarchy/outgoingCalls` → call edges and call-site ranges.
- `textDocument/references` → constant usage and the inferred-edge fallback.
- Rust readiness is checked by asking for one real call-hierarchy item, because rust-analyzer returns empty results (not errors) while the crate graph loads.

Not used: diagnostics (unused variables would duplicate the dead-code heuristic), type hierarchy, semantic tokens, inlay hints. They are valid follow-ups but do not change the graph shape.
