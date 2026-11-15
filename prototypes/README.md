# code-graph — thirty next interface directions

Second design cut. The previous set asked “what could this be?”; this set starts from the ten
directions that survived review and composes them into **30 new interactive prototypes**. Every
page uses the same checkout-service graph: 20 nodes, 23 edges, 2,462 symbols in the surrounding
snapshot. No frameworks, no build step.

```bash
python3 -m http.server 4173 --bind 127.0.0.1
# http://127.0.0.1:4173/
```

## The ten source directions

| Source | What survived | Where it is reused |
|---|---|---|
| **P02** Project overview | Health first; the graph is a destination, not the home page. | N01, N22, N25, N28 |
| **P03** Graph first | Full-bleed canvas; floating chrome; inspector on demand. | N06, N08, N09, N27 |
| **P06** Triage inbox | Task-first, risk-first queue; the graph is a preview. | N11, N15, N20, N23, N29 |
| **P07** Launcher | Keyboard-first search, grouped jumps, recent work. | N02, N03, N04, N08, N19, N29 |
| **P09** Focus + context | One selected node stays central; context recedes; path strip. | N06, N07, N10, N21, N26 |
| **P10** Path story | A request told step by step; playback, code, arrows. | N07, N12, N18, N24, N30 |
| **P11** Blast radius | Reverse impact, risk score, coverage, affected table. | N13, N14, N15, N17, N30 |
| **P13** Linked table | Table primary; hover brushes the graph; click keeps context. | N16, N17, N18, N19, N20, N27 |
| **P19** Glass aurora | Soft translucent surfaces; hierarchy under blur. | N21, N23 |
| **P24** Spatial | Rounded floating surfaces, calm depth, segmented pills. | N05, N22, N24, N26 |

## The 30 directions

### Start — choose where to begin

| # | Prototype | Sources | The bet |
|---|---|---|---|
| N01 | Project pulse | P02 + P06 | Health, risk, recent changes; one obvious graph door. |
| N02 | Command home | P07 + P02 | Search-first home with grouped jumps and project pulse. |
| N03 | Resume desk | P07 + P02 + P09 | Continue from session notes and recent graphs. |
| N04 | Intent launcher | P07 + P06 | Pick the question first, then see matching signals. |
| N05 | Saved spaces | P24 + P07 | Durable maps with mini previews and a new-space door. |

### Graph — stay in the graph

| # | Prototype | Sources | The bet |
|---|---|---|---|
| N06 | Focus lens | P03 + P09 | Full-bleed canvas, path strip, inspector only when selected. |
| N07 | Route theater | P09 + P10 | Play the entry → exit route with the graph as the stage. |
| N08 | Canvas command | P03 + P07 | Floating ⌘K palette over an uninterrupted canvas. |
| N09 | Semantic zoom | P03 + P02 | Architecture, modules, functions: one graph at three scales. |
| N10 | Pinned context | P09 + P13 | Pin two functions and compare context without losing either. |

### Task — start from a question

| # | Prototype | Sources | The bet |
|---|---|---|---|
| N11 | Triage board | P06 + P11 | Risk queue with blast-radius facts in the preview. |
| N12 | Path story | P10 + P09 | Guided reading of a request with code and playback. |
| N13 | Blast radius | P11 + P13 | Reverse callers, risk, coverage, affected table. |
| N14 | Incident trace | P10 + P11 | Trace incident frames on the route; mark the suspect. |
| N15 | Change checklist | P06 + P11 | Review a PR by blast radius, with live checklist. |

### Table — sort, compare, brush

| # | Prototype | Sources | The bet |
|---|---|---|---|
| N16 | Linked table | P13 + P09 | Table primary; hover brushes the graph; click keeps selection. |
| N17 | Impact matrix | P13 + P11 | Sortable risk/reach/coverage with a graph preview. |
| N18 | Path ledger | P13 + P10 | Every route step is a ledger row; the graph follows it. |
| N19 | Function index | P13 + P07 | Search-first function table with a context preview. |
| N20 | Review sheet | P13 + P06 | Table as review checklist with progress and graph context. |

### Visual — same product, different atmosphere

| # | Prototype | Sources | The bet |
|---|---|---|---|
| N21 | Glass focus | P19 + P09 | Aurora glass around a focused canvas; blur only on overlays. |
| N22 | Spatial home | P24 + P02 | Soft floating project health, entries, and recents. |
| N23 | Glass triage | P19 + P06 | Aurora inbox: signals, preview, risk detail. |
| N24 | Spatial route | P24 + P10 | Floating path player with endpoints and code. |
| N25 | Editorial brief | P02 + P11 | Calm article-like reading with evidence and risk box. |

### Adaptive — same ideas under constraints

| # | Prototype | Sources | The bet |
|---|---|---|---|
| N26 | Mobile focus | P09 + P24 | 390 px graph with a bottom inspector sheet. |
| N27 | Split workspace | P03 + P13 | Draggable table/graph split with keyboard-friendly selection. |
| N28 | Overview inspector | P02 + P13 | Overview first; map, table, and inspector together. |
| N29 | Command review | P07 + P06 | Keyboard-only review: filter, arrows, enter, graph follows. |
| N30 | Shareable brief | P10 + P11 | Read-only route + impact artifact with copy/export. |

## Research anchors

- **Overview first, zoom and filter, details on demand** — Ben Shneiderman’s information-seeking
  mantra. N28 makes the order explicit; N06/N09 keep details behind selection.
- **Progressive disclosure** — show the common path first, keep advanced controls one step away.
  [NN/g progressive disclosure](https://www.nngroup.com/articles/progressive-disclosure/).
- **Focus + context** — keep the selected object central and let the rest recede. N06/N21/N26.
- **Command palette anatomy** — trigger, grouped results, hints, keyboard navigation, empty state.
  [UX Patterns: Command Palette](https://uxpatterns.dev/patterns/advanced/command-palette).
- **Graph UX practice** — readable labels, preserved selection, minimaps as wayfinding.
  [Cambridge Intelligence graph UX](https://cambridge-intelligence.com/blog/why-font-icons-are-the-key-to-better-graph-ux-ui/).
- **Code navigation** — symbol search, breadcrumbs, definitions, references are more familiar than
  a free-floating graph. [Sourcegraph code navigation](https://sourcegraph.com/docs/code-navigation).

## What changed from the first cut

- The previous 27 files were removed. Every page now starts from a selected direction, not from a
  blank taxonomy.
- The gallery tags each prototype with its source pair, so “which idea is being tested?” is visible
  before opening it.
- Route playback and entry → exit context are shared data (`CG.checkoutPath`, `graph.route()`),
  not a decorative path graphic.
- Table brushing, palette filtering, signal queues, checklist progress, and the mobile sheet are
  all wired to real DOM state. Nothing is a static screenshot.
- One graph renderer (`assets/proto.js`) and one interaction helper (`assets/next.js`) keep pages
  comparable. Per-page themes stay local.

## Verification

Observed against the static server at 1440, 1024, and 390 px:

- Gallery: **30 tiles**, family filters work (`graph` → 5), search works (`mobile` → 1), no console
  errors.
- All 30 pages load with **zero console errors**.
- No horizontal overflow at 1440, 1024, or 390 px.
- Every graph page mounts the shared graph: 20 nodes / 23 edges; N09 mounts three stages
  (architecture, modules, symbols).
- Smoke interaction pass covered every page: selection, filters, tabs, playback, checklist toggles,
  palette open/select, pinning, route highlighting, and the mobile sheet.
- Responsive fixes verified: linked table at 1024, pinned graph at 390, impact matrix and command
  review at 390.

## Suggested next cut

The strongest composition is probably **N01 as the landing page + N06/N08 as the graph workspace +
N16/N18 as the power-user surface + N12/N30 as shareable explanations**. The pages worth keeping as
separate bets are N04 (intent-first), N09 (semantic zoom), N13 (impact), N21/N24 (visual language),
and N26 (mobile). Avoid combining glass and spatial in the same shell; both rely on atmosphere, and
one will erase the other.
