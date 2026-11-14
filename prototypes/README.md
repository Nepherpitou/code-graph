# code-graph — interface directions

27 interactive prototypes, one realistic graph. Open `index.html` for the gallery.

```bash
python3 -m http.server 4173 --bind 127.0.0.1
# http://127.0.0.1:4173/
```

Every demo uses the same checkout-service graph: 20 nodes, 23 edges, 2,462 symbols in the
surrounding snapshot. Cards are selectable; the inspector and table brushers update from the
same shared renderer. There are no frameworks or build steps.

## Why the current UI feels heavy

The current app is a **three-column console**: all nodes, entry points, server lookup, filters,
insights, and details are mounted at once. All panels have equal visual weight, the graph is the
middle column, and there is no answer to “where do I start?”. The prototypes separate those
concerns along four axes:

| Axis | Question |
|---|---|
| Starting point | Resume, browse, search, or risk-first? |
| Navigation | Breadcrumb, path strip, tree, tabs, palette, or saved spaces? |
| Inspection | Right rail, floating card, bottom sheet, or linked table? |
| Visual language | Dense engineering, editorial, blueprint, terminal, glass, or spatial? |

## Research anchors

- **Overview first, zoom and filter, details on demand** — Ben Shneiderman’s information-seeking
  mantra. The graph should not be the first thing a new user sees in full.
- **Progressive disclosure** — show the common path first; keep advanced commands/filters one step
  away. [NN/g’s progressive disclosure overview](https://www.nngroup.com/articles/progressive-disclosure/).
- **Command palette anatomy** — trigger, search input, grouped results, result hints, empty state,
  keyboard navigation. [UX Patterns: Command Palette](https://uxpatterns.dev/patterns/advanced/command-palette).
- **Graph UX practice** — readable labels beat global overview; selection should preserve context;
  minimaps and legends are wayfinding, not decoration.
  [Cambridge Intelligence graph UX writing](https://cambridge-intelligence.com/blog/why-font-icons-are-the-key-to-better-graph-ux-ui/).
- **Code navigation** — Sourcegraph/GitHub patterns: symbol search, file breadcrumbs, reference
  panels, and “go to definition” are more familiar than a free-floating graph.
  [Sourcegraph code navigation](https://sourcegraph.com/docs/code-navigation).

## Prototype map

| # | Concept | Family | The bet |
|---|---|---|---|
| P01 | Command center | start | Resume + search + three doors; graph is a destination. |
| P02 | Project overview | start | Health, risk, recent changes; one obvious graph CTA. |
| P03 | Graph first | navigate | Full-bleed canvas with floating chrome; maximum graph. |
| P04 | Workspace | navigate | VS Code shell: tree, tabs, graph, outline. Familiar and dense. |
| P05 | Onboarding stepper | start | Connect → analyze → choose a door; explains the slow part once. |
| P06 | Triage inbox | workflow | Risk-first feed; graph is a preview, not the product. |
| P07 | Launcher | start | Keyboard-first search as the home page. |
| P08 | Spaces & tabs | navigate | Saved views as durable spaces with browser-like tabs. |
| P09 | Focus + context | inspect | Selected node stays central; path strip above; everything else recedes. |
| P10 | Call path story | workflow | Play a request through the call chain, step by step. |
| P11 | Blast radius | inspect | Reverse impact columns, risk score, coverage. |
| P12 | Module map | navigate | Start at module boundaries, drill into symbols. |
| P13 | Linked table + graph | inspect | Table is primary; hover brushes the graph. |
| P14 | Command palette | navigate | Palette overlays the graph; jump/actions/filters in one place. |
| P15 | Mobile bottom sheet | navigate | 390 px graph with a bottom inspector sheet. |
| P16 | PR impact review | workflow | Changed files → impacted functions → review checklist. |
| P17 | Editorial light | visual | Article-like explanation with an inline graph. |
| P18 | Neo-brutalist terminal | visual | Monospace, hard borders, acid accent, zero decoration. |
| P19 | Glass aurora | visual | Translucent panels; tests hierarchy under blur. |
| P20 | Swiss monochrome | visual | Typography and grid carry hierarchy; one red accent. |
| P21 | Blueprint | visual | Engineering drawing with schedule, parts list, dimensions. |
| P22 | Paper notebook | visual | Warm paper, notes, sticky questions. |
| P23 | Dense terminal | visual | Power-user table, tiny type, amber highlights, feed. |
| P24 | Spatial / Apple | visual | Soft depth, floating surfaces, segmented pills. |
| P25 | Stack trace debugger | workflow | Paste a trace; highlight the exact execution path. |
| P26 | Architecture review | workflow | Module clusters, risk matrix, checklist, decisions. |
| P27 | Search-first | start | GitHub-style global search with grouped results and preview. |

## Strong ideas to steal

1. **Make the entry point explicit.** P01/P02/P07 all beat the current “graph appears with entry
   points in the left rail”. The best composition is probably P02’s project home + P01’s resume
   cards + P07’s `/` and `⌘K`.
2. **Separate “what to look at” from “the graph”.** P06/P16/P25 show that engineers often arrive
   with a task (a stack trace, a PR, a risk review). The graph is the explanation, not the inbox.
3. **Use a path strip, not a free-floating canvas.** P09/P10 make the current selection legible
   and give back/forward semantics without a browser history mental model.
4. **Inspector on demand.** P03/P09/P15: the right rail should appear when something is selected,
   and disappear when it is not. The current permanent right column spends 356 px on “no selection”.
5. **Linked table + graph** (P13) is the highest-information, lowest-learning-cost combination for
   power users: the table gives sortable, searchable access; the graph explains adjacency.
6. **One accent + one density decision.** P20 and P18 show that strong hierarchy can survive low
   decoration. P19/P24 are attractive but need contrast audits before shipping.
7. **Saved views are durable state.** P08’s “space” model is a better mental model than the current
   transient filters and “Top entries” button.

## Ideas to keep out (or heavily constrain)

- **Glass blur over graph cards** (P19): beautiful in screenshots, but labels lose contrast and
  the graph becomes secondary to the chrome. Keep blur for overlays only.
- **A full-page wizard** (P05) is good for first run, terrible for the 20th run. Use it once, then
  land on P02/P07.
- **Blueprint/paper metaphors** (P21/P22) are memorable but don’t scale to 4,500 nodes or repeated
  daily use. Use them for documentation screenshots, not the primary workspace.
- **Tiny Bloomberg density** (P23) is excellent for experts and unusable for newcomers. If adopted,
  make it an explicit “power mode”, not the default.
- **Too many simultaneous affordances** in the current UI: search, filters, server path, insights,
  all nodes, entry points, details. Every prototype that works has at most one primary navigation
  surface plus one contextual surface.

## Verification

Run against the static server at 1440, 1024, and 390 px:

- All 27 demos have a working `← all prototypes` badge (27/27 navigate to `index.html`).
- No console errors on any demo.
- No horizontal overflow at 390 px on any demo (fixed P01, P09, P10, P12, P20 during this pass).
- Graph renders resize through `ResizeObserver`; scale changes when the shell changes size.
- Interaction checks cover overlays, node selection, table brushing, filters, palette filtering,
  story playback, checklists, command input, tabs, and the mobile sheet.
- Gallery filter/search and card navigation work.

## Suggested next cut

Build the real app around **P02 as the landing page**, **P09/P03 as the graph workspace**,
**P14 as the universal palette**, and **P06/P16/P25 as task-specific entry modes**. Keep the
canvas HTML-card renderer from the current app; the prototypes are about information architecture
and chrome, not rendering technology.
