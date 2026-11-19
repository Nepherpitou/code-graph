#!/usr/bin/env python3
"""Generate candidate PR-report formats from code-graph snapshots of real PRs.

Usage: python3 generate.py [pr-numbers...]
Writes index.html next to this file.
"""
from __future__ import annotations

import json
import os
import re
import sys
from collections import defaultdict

import lib
import md

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "index.html")


# --------------------------------------------------------------------------- #
# formatting helpers
# --------------------------------------------------------------------------- #

def n(value: float, digits: int = 0) -> str:
    if digits:
        return f"{value:.{digits}f}"
    return f"{value:.0f}"


def signed(value: float, digits: int = 0) -> str:
    if abs(value) < 10 ** (-digits) / 2:
        return "0" if not digits else f"{0:.{digits}f}"
    text = f"{value:+.{digits}f}" if digits else f"{value:+.0f}"
    return text


def cell(value: float, digits: int = 0) -> str:
    return f"{value:.{digits}f}" if digits else f"{value:.0f}"


def arc(before: float, after: float, digits: int = 0) -> str:
    """Added and removed symbols show a signed value; both sides show before → after."""
    if before == after:
        return cell(after, digits)
    if before == 0:
        return f"**+{cell(after, digits)}**"
    if after == 0:
        return f"**−{cell(before, digits)}**"
    return f"{cell(before, digits)} → {cell(after, digits)}"


def dcell(before: float, after: float, digits: int = 0) -> str:
    d = after - before
    if d == 0:
        return "0"
    return signed(d, digits)

def is_dispatch_artifact(src: dict, dst: dict, pairs: set) -> bool:
    """Same-named methods are linked by the LSP only when they are overrides or siblings."""
    if src["name"] != dst["name"]:
        return False
    if src["declaration"].lstrip().startswith("abstract") or dst["declaration"].lstrip().startswith("abstract"):
        return True
    return (dst["id"], src["id"]) in pairs


def plural(count: int, one: str, many: str | None = None) -> str:
    return one if count == 1 else (many or one + "s")


def pct(before: float, after: float) -> str:
    if before == 0:
        return "new" if after else "0"
    return f"{signed((after - before) / before * 100, 0)}%"


# --------------------------------------------------------------------------- #
# derived context
# --------------------------------------------------------------------------- #

FRAMEWORK_FILE = re.compile(r"\.(controller|module|provider|guard|interceptor)\.ts$")


class Ctx:
    def __init__(self, pd: lib.PRData):
        self.pd = pd
        self.tb = lib.totals(pd.base_nodes)
        self.ta = lib.totals(pd.head_nodes)
        self.by_status = defaultdict(int)
        for row in pd.rows:
            self.by_status[row.status] += 1
        self.files = lib.file_rollup(pd)
        self.new_dead = [r for r in pd.rows if r.status == "added" and r.after.get("deadCodeCandidate")]
        self.removed_dead = [r for r in pd.rows if r.status == "removed" and r.before.get("deadCodeCandidate")]
        self.boundary = [r for r in pd.rows if r.status in ("changed", "added")
                         and (r.after.get("callFanIn", 0) or r.before.get("callFanIn", 0))]
        self.spikes = sorted((r for r in pd.rows if r.d("cyclomatic") >= 2),
                             key=lambda r: r.d("cyclomatic"), reverse=True)
        self.added = sorted((r for r in pd.rows if r.status == "added"),
                            key=lambda r: r.after.get("cyclomatic", 0), reverse=True)
        self.removed = [r for r in pd.rows if r.status == "removed"]
        self.changed = [r for r in pd.rows if r.status == "changed"]
        self.ranked = sorted(pd.rows, key=lambda r: abs(r.d("refactorPriority")), reverse=True)
        callable_rows = lambda rows: [r for r in rows if r.kind != "constant" and not r.synthetic]
        self.edges = self._edge_changes()
        self.added_profile = self._profile(callable_rows(self.added), "after")
        self.removed_profile = self._profile(callable_rows(self.removed), "before")
        self.areas = self._areas()
        self.cycles_before, self.cycles_after, self.cycle_names = self._cycles()
        touched = {r.file for r in pd.rows}
        self.silent_files = [f for f in pd.changed if f not in touched]
        self.diagnosis = self._diagnose()

    def _diagnose(self):
        pd = self.pd
        notes = []
        added = self.by_status.get("added", 0)
        if added:
            notes.append(f"{added} added {plural(added, 'symbol')}: the shipped format prints \u00ab\u2014\u00bb for their metrics instead of 0 \u2192 N")
        if len(pd.rows) > 12:
            notes.append(f"{len(pd.rows)} rows in one flat table with no grouping and no noise floor")
        framework_dead = [r for r in self.new_dead if FRAMEWORK_FILE.search(r.file)]
        if framework_dead:
            notes.append(f"{len(framework_dead)} of {len(self.new_dead)} new dead-code candidates sit in decorator-registered files "
                         f"(*.controller/*.module/...) \u2014 DI and route decorators are invisible to the call graph, so these are false positives")
        spec_rows = [r for r in pd.rows if r.file.endswith(".spec.ts")]
        if spec_rows:
            notes.append(f"{len(spec_rows)} rows come from *.spec.ts files (test helpers are not production call graph)")
        consts = [r for r in pd.rows if r.kind == "constant"]
        if consts:
            notes.append(f"{len(consts)} {plural(len(consts), 'row')} are constants (zod schemas, config objects): "
                         f"cyclomatic is always 0 there")
        if self.silent_files:
            notes.append(f"{len(self.silent_files)} changed files produce no symbol rows (ports, DTOs, schemas, specs): "
                         + ", ".join(f"`{f}`" for f in self.silent_files[:3])
                         + (f" +{len(self.silent_files) - 3}" if len(self.silent_files) > 3 else ""))
        return notes

    def _edge_changes(self):
        pd = self.pd
        base_ids = {e["id"] for e in pd.base_edges if e["kind"] == "call"}
        base_pairs = set()
        for edge in pd.base_edges:
            if edge["kind"] != "call":
                continue
            src = pd.base_nodes.get(edge["source"])
            dst = pd.base_nodes.get(edge["target"])
            if src and dst:
                base_pairs.add((src["file"], src["qname"], dst["file"], dst["qname"]))
        text_cache = {}
        root_src = f"{lib.DATA}/{pd.number}/head/backend/src"

        import_cache = {}
        import_re = re.compile(r"""(?:from\s+|import\()\s*['"]([^'"]+)['"]""")

        def imports_file(src_path: str, dst_path: str) -> bool:
            if src_path not in import_cache:
                try:
                    text = open(f"{root_src}/{src_path}", encoding="utf-8").read()
                except OSError:
                    text = ""
                resolved = set()
                for match in import_re.finditer(text):
                    spec = match.group(1)
                    if not spec.startswith("."):
                        continue
                    base = os.path.normpath(os.path.join(os.path.dirname(src_path), spec))
                    resolved.update((base + ".ts", base + "/index.ts"))
                import_cache[src_path] = resolved
            if dst_path in import_cache[src_path]:
                return True
            # same directory or barrel re-export through the module index
            return os.path.dirname(src_path) == os.path.dirname(dst_path)

        def line_mentions(path: str, line: int, name: str) -> bool:
            if path not in text_cache:
                try:
                    text_cache[path] = open(f"{root_src}/{path}", encoding="utf-8").read().split("\n")
                except OSError:
                    text_cache[path] = []
            lines = text_cache[path]
            window = " ".join(lines[max(0, line - 2):line + 1])
            # A call, or a property reference to a getter: `this.config.xqAiModel`.
            return f"{name}(" in window or f"{name} (" in window or f".{name}" in window

        pairs = {(e["source"], e["target"]) for e in pd.head_edges if e["kind"] == "call"}
        changes = {"real": [], "spurious": [], "test": [], "wiring": [], "dup": [], "resignature": [], "unverified": []}
        candidates = []
        for edge in pd.head_edges:
            if edge["kind"] != "call" or edge["id"] in base_ids:
                continue
            src = pd.head_nodes.get(edge["source"])
            dst = pd.head_nodes.get(edge["target"])
            if not src or not dst:
                continue
            item = {"src": src, "dst": dst, "edge": edge}
            files = (src["file"], dst["file"])
            if is_dispatch_artifact(src, dst, pairs):
                # Overrides of a base method are linked to each other by the LSP call
                # hierarchy; a same-named call between unrelated classes is real.
                changes["spurious"].append(item)
            elif any(f.endswith(".spec.ts") or "/test/" in f for f in files):
                changes["test"].append(item)
            elif "constructor" in (src["kind"], dst["kind"]):
                # DI wiring: module imports and injection, not a call.
                changes["wiring"].append(item)
            else:
                candidates.append(item)

        # One call site can resolve to both an abstract port method and its implementation.
        # Keep the abstraction, drop the implementation; dispatch fan-out is kept in full.
        by_site = {}
        for item in candidates:
            sites = item["edge"].get("callSites") or ()
            line = min((cs.get("line") or 0) for cs in sites) if sites else 0
            by_site.setdefault((item["edge"]["source"], line), []).append(item)
        for (_, line), group in by_site.items():
            abstract = {item["dst"]["name"] for item in group
                        if item["dst"]["declaration"].lstrip().startswith("abstract")}
            for item in group:
                dst = item["dst"]
                if dst["name"] in abstract and not dst["declaration"].lstrip().startswith("abstract"):
                    changes["dup"].append(item)
                    continue
                if (item["src"]["file"], item["src"]["qname"],
                        dst["file"], dst["qname"]) in base_pairs:
                    # Same call already existed: only the callee signature changed.
                    changes["resignature"].append(item)
                    continue
                if not line_mentions(item["src"]["file"], line, dst["name"]) \
                        and not imports_file(item["src"]["file"], dst["file"]):
                    # The line never mentions this name and the file does not import the
                    # callee: the LSP resolved a local variable to a same-named declaration.
                    changes["unverified"].append(item)
                    continue
                changes["real"].append(item)
        return changes

    def module_pairs(self):
        from collections import Counter
        pairs = Counter()
        for edge in self.edges["real"]:
            a1 = edge["src"]["file"].split("/")[0]
            a2 = edge["dst"]["file"].split("/")[0]
            if a1 != a2:
                pairs[(a1, a2)] += 1
        return pairs

    def call_edges(self, side: str) -> int:
        edges = self.pd.base_edges if side == "base" else self.pd.head_edges
        return sum(1 for e in edges if e["kind"] == "call")

    def cross_module(self, side: str) -> int:
        nodes = self.pd.base_nodes if side == "base" else self.pd.head_nodes
        edges = self.pd.base_edges if side == "base" else self.pd.head_edges
        count = 0
        for edge in edges:
            if edge["kind"] != "call":
                continue
            src, dst = nodes.get(edge["source"]), nodes.get(edge["target"])
            if not src or not dst or src["name"] == dst["name"]:
                continue
            if src["file"].split("/")[0] != dst["file"].split("/")[0]:
                count += 1
        return count

    def area_avg_cx(self, area: str) -> str:
        """Mean cyclomatic of the area's callables, before and after."""
        def in_area(node):
            parts = node["file"].split("/")
            return (parts[0] if len(parts) > 1 else node["file"]) == area

        def mean(nodes):
            values = [n.get("cyclomatic", 0) for n in nodes.values()
                      if in_area(n) and n["kind"] in ("function", "method") and not n.get("synthetic")]
            return sum(values) / len(values) if values else None

        before, after = mean(self.pd.base_nodes), mean(self.pd.head_nodes)
        if before is None and after is None:
            return "—"
        if before is None:
            return f"+{after:.2f}"
        if after is None:
            return f"−{before:.2f}"
        if abs(after - before) < 0.005:
            return f"{after:.2f}"
        return f"{before:.2f} → {after:.2f}"

    def reach(self) -> int:
        """How many symbols outside the changed files can reach them through calls."""
        pd = self.pd
        rev, _ = self.call_graph()
        changed = set(pd.changed)
        seeds = {nid for nid, node in pd.head_nodes.items() if node["file"] in changed}
        seen = set(seeds)
        stack = list(seeds)
        while stack:
            for caller in rev.get(stack.pop(), ()):
                if caller not in seen:
                    seen.add(caller)
                    stack.append(caller)
        return len(seen - seeds)

    def skipped_rows(self) -> int:
        return sum(1 for r in self.pd.rows
                   if r.kind == "constant" or r.synthetic or r.file.endswith(".spec.ts"))

    def mutual_pairs(self):
        pairs = self.module_pairs()
        return sorted({tuple(sorted(k)) for k in pairs if (k[1], k[0]) in pairs})

    def call_graph(self):
        if getattr(self, "_graph", None):
            return self._graph
        nodes = self.pd.head_nodes
        rev, fwd = defaultdict(list), defaultdict(list)
        pairs = {(e["source"], e["target"]) for e in self.pd.head_edges if e["kind"] == "call"}
        for edge in self.pd.head_edges:
            if edge["kind"] != "call":
                continue
            src, dst = nodes.get(edge["source"]), nodes.get(edge["target"])
            if not src or not dst or is_dispatch_artifact(src, dst, pairs):
                continue
            if "constructor" in (src["kind"], dst["kind"]):
                continue
            if src["file"].endswith(".spec.ts") or dst["file"].endswith(".spec.ts"):
                continue
            rev[dst["id"]].append(src["id"])
            fwd[src["id"]].append(dst["id"])
        self._graph = (rev, fwd)
        return self._graph

    def affected_existing(self, limit: int = 8):
        """Existing callables that reach changed code, directly or transitively."""
        rev, fwd = self.call_graph()
        pd = self.pd
        changed_files = set(pd.changed)
        touched = {}
        for row in pd.rows:
            if row.kind not in ("function", "method") or row.status not in ("added", "changed", "moved"):
                continue
            touched[(row.file, row.qname)] = row.status
        touched_ids = {nid: status for nid, node in pd.head_nodes.items()
                       if (node["file"], node["qname"]) in touched
                       for status in (touched[(node["file"], node["qname"])],)}

        counts = defaultdict(lambda: {"changed": 0, "added": 0})
        for cid, status in touched_ids.items():
            seen = {cid}
            stack = [cid]
            while stack:
                for caller in rev.get(stack.pop(), ()):
                    if caller in seen:
                        continue
                    seen.add(caller)
                    stack.append(caller)
            bucket = "added" if status == "added" else "changed"
            for nid in seen:
                if nid != cid and pd.head_nodes[nid]["file"] not in changed_files:
                    counts[nid][bucket] += 1

        def priority(nid):
            return pd.head_nodes[nid]["metrics"].get("refactorPriority", 0.0)

        rows = sorted(counts.items(),
                      key=lambda kv: (-kv[1]["changed"], -kv[1]["added"], -priority(kv[0])))
        direct = [nid for nid in counts if touched_ids.keys() & set(fwd.get(nid, ()))]
        out = []
        for nid, kinds in rows[:limit]:
            node = pd.head_nodes[nid]
            seen = {nid}
            stack = [nid]
            while stack:
                for callee in fwd.get(stack.pop(), ()):
                    if callee not in seen:
                        seen.add(callee)
                        stack.append(callee)
            total = max(1, len(seen) - 1)
            out.append({
                "qname": node["qname"],
                "changed": kinds["changed"],
                "added": kinds["added"],
                "all_callees": total,
                "priority": priority(nid),
                "direct": nid in direct,
            })
        affected_changed = sum(1 for kinds in counts.values() if kinds["changed"])
        affected_added_only = sum(1 for kinds in counts.values() if not kinds["changed"])
        return len(counts), affected_changed, affected_added_only, len(direct), out

    def incoming_edges(self, limit: int = 40):
        """New edges from existing code into the changed files."""
        changed = set(self.pd.changed)
        out = []
        for edge in self.edges["real"]:
            if edge["dst"]["file"] in changed and edge["src"]["file"] not in changed:
                out.append(edge)
        out.sort(key=lambda e: (e["dst"]["qname"], e["src"]["qname"]))
        return out[:limit]

    def hubs_gained(self, limit: int = 8):
        counts = defaultdict(int)
        base_ids = set(self.pd.base_nodes)
        for edge in self.edges["real"]:
            dst = edge["dst"]
            if dst["id"] not in base_ids:
                continue
            if dst["kind"] not in ("function", "method") or dst.get("synthetic"):
                continue
            if dst["file"].endswith(".spec.ts") or FRAMEWORK_FILE.search(dst["file"]):
                continue
            if dst["file"].startswith(("common/decorators/", "common/pipes/", "common/dto/")):
                continue
            counts[(dst["qname"], dst["file"])] += 1
        return sorted(counts.items(), key=lambda kv: -kv[1])[:limit]

    def orphans(self):
        out = []
        for r in self.pd.rows:
            if r.status != "added" or r.kind not in ("function", "method") or r.synthetic:
                continue
            if r.file.endswith(".spec.ts") or "/test/" in r.file or FRAMEWORK_FILE.search(r.file):
                continue
            if not r.after.get("callFanIn") and not r.after.get("callFanOut"):
                out.append(r)
        return out

    def exports(self):
        added = [r for r in self.pd.rows if r.status == "added" and r.exported
                 and r.kind in ("function", "method", "constant") and not r.synthetic]
        removed = [r for r in self.pd.rows if r.status == "removed" and r.exported
                   and r.kind in ("function", "method", "constant") and not r.synthetic]
        return added, removed

    @staticmethod
    def _profile(rows, side):
        if not rows:
            return None
        cx = sorted(r.__dict__[side].get("cyclomatic", 0) for r in rows)
        loc = sorted(r.__dict__[side].get("loc", 0) for r in rows)
        pick = lambda seq, q: seq[min(len(seq) - 1, int(q * (len(seq) - 1)))]
        return {
            "count": len(rows),
            "cx_median": pick(cx, 0.5),
            "cx_p90": pick(cx, 0.9),
            "cx_max": cx[-1],
            "loc_total": sum(loc),
            "loc_max": loc[-1],
            "with_callers": sum(1 for r in rows if r.__dict__[side].get("callFanIn", 0)),
        }

    def _areas(self):
        groups = defaultdict(lambda: {"added": 0, "removed": 0, "changed": 0, "moved": 0,
                                      "d_cx": 0, "d_loc": 0, "new_dead": 0, "d_priority": 0.0})
        for row in self.pd.rows:
            area = row.file.split("/", 1)[0] if "/" in row.file else row.file
            a = groups[area]
            a[row.status] += 1
            a["d_cx"] += row.d("cyclomatic")
            a["d_loc"] += row.d("loc")
            a["d_priority"] += row.d("refactorPriority")
            if row.status == "added" and row.after.get("deadCodeCandidate"):
                a["new_dead"] += 1
        out = [{"area": k, **v} for k, v in groups.items()]
        out.sort(key=lambda a: (-abs(a["d_cx"]), -abs(a["d_priority"])))
        return out

    def _cycles(self):
        pd = self.pd
        nodes = {**pd.base_nodes, **pd.head_nodes}

        def real(comp):
            # Overrides of one base method (run/handle/...) get linked to each other by the
            # LSP call hierarchy: a cycle made of same-named methods is a dispatch artifact.
            names = {nodes[k]["name"] for k in comp if k in nodes}
            return len(names) > 1

        cb = {frozenset(c) for c in lib.cycles_of(pd.base_nodes, pd.base_edges) if real(c)}
        ca = {frozenset(c) for c in lib.cycles_of(pd.head_nodes, pd.head_edges) if real(c)}
        names = lambda comp: sorted(nodes[k]["qname"] for k in comp if k in nodes)
        return cb, ca, names

    def failures(self):
        out = []
        if self.by_status.get("added") and all(r.status == "added" or abs(r.d("refactorPriority")) < 1e9
                                               for r in self.pd.rows):
            pass
        added_no_delta = [r for r in self.pd.rows if r.status == "added"]
        if added_no_delta:
            out.append(f"{len(added_no_delta)} added symbols carry no baseline metrics in `audit diff` output")
        if len(self.pd.rows) > 20:
            out.append(f"{len(self.pd.rows)} rows in one flat table")
        return out


# --------------------------------------------------------------------------- #
# shared blocks
# --------------------------------------------------------------------------- #

def header(pd: lib.PRData, level: str = "##") -> str:
    return f"{level} code-graph: {pd.title}\n"


def impact_table(pd: lib.PRData, ctx: Ctx) -> str:
    b, a = pd.base_stats, pd.head_stats
    rows = [
        ("symbols", "nodes", b["nodes"], a["nodes"]),
        ("functions", "functions", b["functions"], a["functions"]),
        ("call/data edges", "edges", b["edges"], a["edges"]),
        ("real cycles", None, len(ctx.cycles_before), len(ctx.cycles_after)),
        ("dead-code candidates", "deadCode", b["deadCode"], a["deadCode"]),
        ("total cyclomatic", None, ctx.tb["complexity"], ctx.ta["complexity"]),
        ("total loc", None, ctx.tb["loc"], ctx.ta["loc"]),
        ("total refactor priority", None, ctx.tb["priority"], ctx.ta["priority"]),
    ]
    lines = ["| metric | before | after | Δ |", "|---|---:|---:|---:|"]
    for label, key, before, after in rows:
        lines.append(f"| {label} | {n(before)} | {n(after)} | {signed(after - before)} |")
    cross_b = b["crossFileRatio"] * 100
    cross_a = a["crossFileRatio"] * 100
    lines.append(f"| cross-file edge share | {cross_b:.1f}% | {cross_a:.1f}% | {signed(cross_a - cross_b, 1)} pp |")
    return "\n".join(lines)


def row_metric(r: lib.Row, key: str) -> tuple[float, float]:
    return r.before.get(key, 0), r.after.get(key, 0)


def symbol_table(rows: list[lib.Row], limit: int | None, columns: list[tuple[str, str, int]],
                 sort_key=None) -> str:
    rows = list(rows)
    if sort_key:
        rows.sort(key=sort_key, reverse=True)
    shown = rows if limit is None else rows[:limit]
    head = "| symbol | file | " + " | ".join(label for label, _, _ in columns) + " | flags |"
    sep = "|---|---|" + "".join("---:|" for _ in columns) + "---|"
    lines = [head, sep]
    for r in shown:
        cells = []
        for _, key, digits in columns:
            before, after = row_metric(r, key)
            cells.append(dcell(before, after) if r.status in ("changed", "moved") else signed(after - before, digits))
        flags = ", ".join(r.flags)
        lines.append(f"| `{r.qname}` | `{r.file}` | " + " | ".join(cells) + f" | {flags} |")
    if limit is not None and len(rows) > limit:
        lines.append(f"| _… {len(rows) - limit} more_ | | | | | | |")
    return "\n".join(lines)


def status_counts(ctx: Ctx) -> str:
    parts = []
    for status in ("added", "removed", "changed", "moved"):
        if ctx.by_status.get(status):
            parts.append(f"{ctx.by_status[status]} {status}")
    return ", ".join(parts) if parts else "no symbol changes"


# --------------------------------------------------------------------------- #
# variants
# --------------------------------------------------------------------------- #

def v_current(pd, ctx):
    """The format the workflow ships today."""
    out = [header(pd)]
    out.append(f"**Snapshots:** `#{1}` → `#{2}` · **rows:** {pd.total_rows}\n")
    if not pd.rows:
        out.append("_No significant call-graph changes in the touched files._")
        return "\n".join(out)
    out.append("| symbol | file | status | Δpriority | Δcomplexity | flags |")
    out.append("|---|---|---|---:|---:|---|")
    for r in ctx.ranked[:20]:
        dp = r.d("refactorPriority")
        dc = r.d("cyclomatic")
        out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | "
                   + (f"{dp:.2f}" if r.before else "—") + " | "
                   + (f"{dc:+.0f}" if r.before else "—") + " | "
                   + ", ".join(r.flags) + " |")
    if len(pd.rows) > 20:
        out.append(f"\n_… {len(pd.rows) - 20} more rows_")
    return "\n".join(out)


def v_absolute(pd, ctx):
    """Same table, but every row shows before → after instead of an em dash."""
    out = [header(pd)]
    out.append(f"**{status_counts(ctx)}** · {pd.additions}+ / {pd.deletions}- lines in {len(pd.changed)} backend files\n")
    out.append("| symbol | file | status | complexity | loc | priority | flags |")
    out.append("|---|---|---|---:|---:|---:|---|")
    for r in ctx.ranked[:25]:
        out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | "
                   + arc(*row_metric(r, "cyclomatic")) + " | "
                   + arc(*row_metric(r, "loc")) + " | "
                   + arc(*row_metric(r, "refactorPriority"), 2) + " | "
                   + ", ".join(r.flags) + " |")
    if len(pd.rows) > 25:
        out.append(f"\n_… {len(pd.rows) - 25} more rows_")
    return "\n".join(out)


def v_impact(pd, ctx):
    """Project impact first, then the symbol table."""
    out = [header(pd)]
    out.append(impact_table(pd, ctx))
    out.append("")
    out.append(f"**Changed symbols:** {status_counts(ctx)} in {len(pd.changed)} files")
    out.append("")
    out.append("| symbol | file | status | complexity | Δloc | Δpriority |")
    out.append("|---|---|---|---:|---:|---:|")
    for r in ctx.ranked[:15]:
        out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | "
                   + arc(*row_metric(r, "cyclomatic")) + " | "
                   + dcell(*row_metric(r, "loc")) + " | "
                   + dcell(*row_metric(r, "refactorPriority"), 2) + " |")
    return "\n".join(out)


def v_grouped(pd, ctx):
    """Added / removed / changed sections."""
    out = [header(pd)]
    if ctx.added:
        out.append(f"### Added ({len(ctx.added)})\n")
        out.append("| symbol | file | complexity | loc | callers | dead? |")
        out.append("|---|---|---:|---:|---:|---|")
        for r in ctx.added[:15]:
            out.append(f"| `{r.qname}` | `{r.file}` | {cell(r.after.get('cyclomatic', 0))} | "
                       f"{cell(r.after.get('loc', 0))} | {cell(r.after.get('callFanIn', 0))} | "
                       f"{'yes' if r.after.get('deadCodeCandidate') else ''} |")
        if len(ctx.added) > 15:
            out.append(f"\n_… {len(ctx.added) - 15} more added_")
    if ctx.removed:
        out.append(f"\n### Removed ({len(ctx.removed)})\n")
        out.append("| symbol | file | complexity | loc |")
        out.append("|---|---|---:|---:|")
        for r in ctx.removed[:15]:
            out.append(f"| `{r.qname}` | `{r.file}` | {cell(r.before.get('cyclomatic', 0))} | "
                       f"{cell(r.before.get('loc', 0))} |")
    if ctx.changed:
        out.append(f"\n### Changed ({len(ctx.changed)})\n")
        out.append("| symbol | file | complexity | Δloc | Δpriority | flags |")
        out.append("|---|---|---|---:|---:|---|")
        for r in sorted(ctx.changed, key=lambda r: abs(r.d("refactorPriority")), reverse=True)[:15]:
            out.append(f"| `{r.qname}` | `{r.file}` | {arc(*row_metric(r, 'cyclomatic'))} | "
                       f"{dcell(*row_metric(r, 'loc'))} | {dcell(*row_metric(r, 'refactorPriority'), 2)} | "
                       f"{', '.join(r.flags)} |")
    return "\n".join(out)


def v_files(pd, ctx):
    """File rollup with symbols folded into details."""
    out = [header(pd)]
    out.append(f"{pd.additions}+ / {pd.deletions}- in {len(pd.changed)} files · {status_counts(ctx)}\n")
    out.append("| file | added | removed | changed | Δcomplexity | Δloc | new dead |")
    out.append("|---|---:|---:|---:|---:|---:|---:|")
    for f in ctx.files[:12]:
        out.append(f"| `{f['file']}` | {f['added']} | {f['removed']} | {f['changed']} | "
                   f"{signed(f['d_complexity'])} | {signed(f['d_loc'])} | {f['new_dead'] or ''} |")
    if len(ctx.files) > 12:
        out.append(f"| _… {len(ctx.files) - 12} more files_ | | | | | | |")
    if pd.rows:
        out.append("<details><summary>symbols</summary>\n")
        out.append("")
        out.append("| symbol | file | status | complexity | Δpriority |")
        out.append("|---|---|---|---:|---:|")
        for r in ctx.ranked[:40]:
            out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | {arc(*row_metric(r, 'cyclomatic'))} | "
                       f"{dcell(*row_metric(r, 'refactorPriority'), 2)} |")
        out.append("\n</details>")
    return "\n".join(out)


def v_signal(pd, ctx):
    """Only rows a reviewer must look at; everything else is a count."""
    out = [header(pd)]
    signal = [r for r in pd.rows if abs(r.d("refactorPriority")) >= 5
              or abs(r.d("cyclomatic")) >= 2 or r.status in ("removed",)]
    out.append(f"{status_counts(ctx)} · {len(signal)} of {len(pd.rows)} rows cross the noise floor "
               f"(Δpriority ≥ 5 or Δcomplexity ≥ 2 or removed)\n")
    if signal:
        out.append("| symbol | file | status | complexity | Δloc | Δpriority |")
        out.append("|---|---|---|---:|---:|---:|")
        for r in sorted(signal, key=lambda r: abs(r.d("refactorPriority")), reverse=True)[:15]:
            out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | {arc(*row_metric(r, 'cyclomatic'))} | "
                       f"{arc(*row_metric(r, 'loc'))} | {arc(*row_metric(r, 'refactorPriority'), 1)} |")
    else:
        out.append("_Nothing above the noise floor._")
    out.append("")
    out.append(f"Project: {n(ctx.tb['priority'])} → {n(ctx.ta['priority'])} priority, "
               f"{n(ctx.tb['complexity'])} → {n(ctx.ta['complexity'])} cyclomatic, "
               f"{pd.base_stats['cycles']} → {pd.head_stats['cycles']} cycles")
    return "\n".join(out)


def v_risk(pd, ctx):
    """Risk-oriented sections."""
    out = [header(pd)]
    cb, ca, names = ctx.cycles_before, ctx.cycles_after, ctx.cycle_names
    new_cycles = [c for c in ca if c not in cb]
    out.append("### Risks\n")
    items = []
    if ctx.new_dead:
        items.append(f"**{len(ctx.new_dead)} new dead-code candidates**: "
                     + ", ".join(f"`{r.qname}`" for r in ctx.new_dead[:5])
                     + (f" (+{len(ctx.new_dead) - 5})" if len(ctx.new_dead) > 5 else ""))
    if new_cycles:
        items.append(f"**{len(new_cycles)} new cycle(s)**: "
                     + "; ".join(" → ".join(names(c)[:4]) for c in new_cycles[:3]))
    if ctx.spikes:
        items.append(f"**{len(ctx.spikes)} complexity spikes**: "
                     + ", ".join(f"`{r.qname}` {signed(r.d('cyclomatic'))}" for r in ctx.spikes[:5]))
    if ctx.removed:
        items.append(f"**{len(ctx.removed)} removed symbols** — check callers are gone")
    if not items:
        items.append("_No structural risks detected._")
    out.append("\n".join(f"- {i}" for i in items))
    out.append("")
    out.append("### Headline numbers\n")
    out.append(impact_table(pd, ctx))
    return "\n".join(out)


def v_boundary(pd, ctx):
    """Interface surface: what other code calls into the changed symbols."""
    out = [header(pd)]
    out.append(f"{len(ctx.boundary)} changed or added symbols are called from {len(ctx.pd.changed)} touched files\n")
    if ctx.boundary:
        out.append("| symbol | file | status | callers | Δcallers | cross-file? |")
        out.append("|---|---|---|---:|---:|---|")
        for r in sorted(ctx.boundary, key=lambda r: r.after.get("transitiveFanIn", 0), reverse=True)[:15]:
            b, a = row_metric(r, "callFanIn")
            out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | {cell(a)} | {signed(a - b)} | "
                       f"{'yes' if r.after.get('callFanIn', 0) else ''} |")
    out.append("")
    out.append(f"Graph reach: transitive fan-in {n(sum(r.before.get('transitiveFanIn', 0) for r in pd.rows))} → "
               f"{n(sum(r.after.get('transitiveFanIn', 0) for r in pd.rows))}")
    return "\n".join(out)


def v_compact(pd, ctx):
    """Three lines, top three movers."""
    out = [header(pd)]
    out.append(f"- **Symbols:** {status_counts(ctx)} across {len(pd.changed)} files "
               f"(+{pd.additions}/-{pd.deletions} lines)")
    out.append(f"- **Project:** complexity {signed(ctx.ta['complexity'] - ctx.tb['complexity'])}, "
               f"priority {signed(ctx.ta['priority'] - ctx.tb['priority'])}, "
               f"cycles {pd.base_stats['cycles']} → {pd.head_stats['cycles']}, "
               f"dead code {pd.base_stats['deadCode']} → {pd.head_stats['deadCode']}")
    top = ctx.ranked[:3]
    if top:
        out.append("- **Top movers:** " + "; ".join(
            f"`{r.qname}` ({r.status}, Δcx {signed(r.d('cyclomatic'))}, "
            f"Δp {signed(r.d('refactorPriority'), 1)})" for r in top))
    if ctx.new_dead:
        out.append(f"- **New dead code:** {len(ctx.new_dead)} candidates")
    return "\n".join(out)


def v_full(pd, ctx):
    """Full metric columns with absolute values."""
    out = [header(pd)]
    out.append("### Project\n")
    out.append(impact_table(pd, ctx))
    out.append("\n### Symbols\n")
    out.append("| symbol | file | status | cx | loc | fan-in | fan-out | ifc | priority |")
    out.append("|---|---|---|---:|---:|---:|---:|---:|---:|")
    for r in ctx.ranked[:15]:
        out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | "
                   + arc(*row_metric(r, "cyclomatic")) + " | "
                   + arc(*row_metric(r, "loc")) + " | "
                   + arc(*row_metric(r, "callFanIn")) + " | "
                   + arc(*row_metric(r, "callFanOut")) + " | "
                   + arc(*row_metric(r, "ifc")) + " | "
                   + arc(*row_metric(r, "refactorPriority"), 1) + " |")
    return "\n".join(out)


def v_delta(pd, ctx):
    """Delta-only numbers: added symbols contribute their absolute values."""
    out = [header(pd)]
    out.append(f"{status_counts(ctx)} in {len(pd.changed)} files\n")
    out.append("| symbol | file | status | Δcx | Δloc | Δfan-in | Δifc | Δpriority |")
    out.append("|---|---|---|---:|---:|---:|---:|---:|")
    for r in ctx.ranked[:20]:
        out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | "
                   f"{dcell(*row_metric(r, 'cyclomatic'))} | {dcell(*row_metric(r, 'loc'))} | "
                   f"{dcell(*row_metric(r, 'callFanIn'))} | {dcell(*row_metric(r, 'ifc'))} | "
                   f"{dcell(*row_metric(r, 'refactorPriority'), 1)} |")
    return "\n".join(out)


def v_checklist(pd, ctx):
    """Reviewer checklist derived from the graph."""
    out = [header(pd)]
    out.append(f"### Review checklist\n")
    checks = []
    if ctx.added:
        biggest = ctx.added[0]
        checks.append(f"- [ ] {len(ctx.added)} new {ctx.added[0].kind}s; largest is "
                      f"`{biggest.qname}` (cx {cell(biggest.after.get('cyclomatic', 0))}, "
                      f"loc {cell(biggest.after.get('loc', 0))})")
    if ctx.spikes:
        checks.append("- [ ] complexity grew: " + ", ".join(
            f"`{r.qname}` {signed(r.d('cyclomatic'))}" for r in ctx.spikes[:4]))
    if ctx.new_dead:
        checks.append(f"- [ ] {len(ctx.new_dead)} new dead-code candidates — confirm they are not DI/decorator registrations")
    if ctx.removed:
        checks.append(f"- [ ] {len(ctx.removed)} removals: " + ", ".join(f"`{r.qname}`" for r in ctx.removed[:4]))
    if ctx.boundary:
        top = sorted(ctx.boundary, key=lambda r: r.after.get('callFanIn', 0), reverse=True)[0]
        checks.append(f"- [ ] boundary churn: `{top.qname}` called by {cell(top.after.get('callFanIn', 0))} functions")
    if not checks:
        checks.append("- [ ] nothing structural changed")
    out.append("\n".join(checks))
    return "\n".join(out)


def v_areas(pd, ctx):
    """Subsystem rollup: where the change lands in the architecture."""
    out = [header(pd)]
    out.append(f"{status_counts(ctx)} in {len(pd.changed)} files\n")
    out.append("| area | added | removed | changed | Δcomplexity | Δloc | new dead |")
    out.append("|---|---:|---:|---:|---:|---:|---:|")
    for a in ctx.areas:
        out.append(f"| `{a['area']}/` | {a['added']} | {a['removed']} | {a['changed']} | "
                   f"{signed(a['d_cx'])} | {signed(a['d_loc'])} | {a['new_dead'] or ''} |")
    out.append("")
    out.append(f"Graph: {pd.base_stats['nodes']} → {pd.head_stats['nodes']} symbols, "
               f"cycles {pd.base_stats['cycles']} → {pd.head_stats['cycles']}, "
               f"dead code {pd.base_stats['deadCode']} → {pd.head_stats['deadCode']}")
    return "\n".join(out)


def v_folded(pd, ctx):
    """One-screen summary; details behind a fold."""
    out = [header(pd)]
    out.append(f"**{status_counts(ctx)}** · {pd.additions}+ / {pd.deletions}- · "
               f"complexity {signed(ctx.ta['complexity'] - ctx.tb['complexity'])} · "
               f"dead code {signed(pd.head_stats['deadCode'] - pd.base_stats['deadCode'])}\n")
    risky = ctx.new_dead + ctx.spikes
    if risky:
        out.append("Attention: " + "; ".join(
            f"`{r.qname}` ({'new dead' if r in ctx.new_dead else 'Δcx ' + signed(r.d('cyclomatic'))})"
            for r in risky[:5]) + "\n")
    out.append("<details><summary>All changed symbols</summary>\n")
    out.append("")
    out.append("| symbol | file | status | cx | loc | callers |")
    out.append("|---|---|---|---:|---:|---:|")
    for r in ctx.ranked[:80]:
        out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | {arc(*row_metric(r, 'cyclomatic'))} | "
                   f"{arc(*row_metric(r, 'loc'))} | {arc(*row_metric(r, 'callFanIn'))} |")
    out.append("\n</details>")
    return "\n".join(out)


def v_hot(pd, ctx):
    """Hotspots the change creates or heats up (absolute, not delta)."""
    out = [header(pd)]
    touched = [r for r in pd.rows if r.status != "removed"]
    hottest = sorted(touched, key=lambda r: r.after.get("refactorPriority", 0), reverse=True)[:10]
    out.append("### Current hotspots in touched files\n")
    out.append("| symbol | file | priority | complexity | callers |")
    out.append("|---|---|---:|---:|---:|")
    for r in hottest:
        out.append(f"| `{r.qname}` | `{r.file}` | {n(r.after.get('refactorPriority', 0), 1)} | "
                   f"{cell(r.after.get('cyclomatic', 0))} | {cell(r.after.get('callFanIn', 0))} |")
    return "\n".join(out)


def v_narrative(pd, ctx):
    """Prose summary; tables hidden."""
    out = [header(pd)]
    out.append(f"This PR touches {len(pd.changed)} backend files (+{pd.additions}/-{pd.deletions}) and "
               f"{status_counts(ctx)}. The graph went from {pd.base_stats['nodes']} to {pd.head_stats['nodes']} "
               f"symbols and {pd.base_stats['edges']} to {pd.head_stats['edges']} edges. "
               f"Total cyclomatic complexity moved by {signed(ctx.ta['complexity'] - ctx.tb['complexity'])} "
               f"({pct(ctx.tb['complexity'], ctx.ta['complexity'])}), refactor priority by "
               f"{signed(ctx.ta['priority'] - ctx.tb['priority'])}.")
    if ctx.new_dead:
        out.append(f"\n{len(ctx.new_dead)} new symbols look unreferenced; the main ones are "
                   + ", ".join(f"`{r.qname}`" for r in ctx.new_dead[:3]) + ".")
    if ctx.spikes:
        out.append(f"\nComplexity rose most in " + ", ".join(
            f"`{r.qname}` ({signed(r.d('cyclomatic'))})" for r in ctx.spikes[:3]) + ".")
    return "\n".join(out)


def v_final(pd, ctx):
    """Best-of draft: impact, review queue, area rollup, folded tail."""
    out = [header(pd)]
    out.append(f"`{pd.head_sha}` · +{pd.additions}/−{pd.deletions} lines in {len(pd.changed)} files · "
               f"{ctx.by_status.get('added', 0)} added, {ctx.by_status.get('removed', 0)} removed, "
               f"{ctx.by_status.get('changed', 0)} changed")
    out.append("")
    out.append(impact_table(pd, ctx).split("\n")[0])
    impact_rows = impact_table(pd, ctx).split("\n")[1:]
    keep = ("symbols", "cycles", "total cyclomatic", "total loc", "cross-file edge share")
    out.append(impact_rows[0])
    for line in impact_rows[1:]:
        if any(line.startswith(f"| {name} ") for name in keep):
            out.append(line)
    out.append("")

    real_dead = {id(r) for r in ctx.new_dead if not FRAMEWORK_FILE.search(r.file)}
    review = []
    skipped = []
    notable_added, notable_changed, notable_removed = [], [], []
    for r in ctx.ranked:
        if r.kind == "constant" or r.synthetic or r.file.endswith(".spec.ts"):
            skipped.append(r)
            continue
        if r.status == "removed":
            notable_removed.append(r)
        elif r.status == "added":
            notable_added.append(r)
        elif abs(r.d("cyclomatic")) >= 1 or abs(r.d("refactorPriority")) >= 5:
            notable_changed.append(r)
    notable_added.sort(key=lambda r: (r.after.get("cyclomatic", 0), r.after.get("loc", 0)), reverse=True)
    review = (notable_removed[:3] + notable_added[:6] + notable_changed[:5])
    seen = set()
    review = [r for r in review if not (id(r) in seen or seen.add(id(r)))]
    review = review[:12]
    prof = ctx.added_profile
    if prof:
        out.append(f"{prof['count']} new {plural(prof['count'], 'symbol')} "
                   f"({prof['loc_total']} lines; cyclomatic median {prof['cx_median']:.0f}, "
                   f"max {prof['cx_max']:.0f}); {prof['with_callers']} called from existing code.")
        out.append("")
    note = f"{len(skipped)} constants, synthetic module constructors and test-file rows left out of the list" if skipped else ""
    out.append(f"### Review these ({len(review)} of {len(pd.rows)})\n")
    if note:
        out.append(f"_{note}._\n")
    if review:
        out.append("| symbol | file | status | complexity | loc | callers | priority | why |")
        out.append("|---|---|---|---:|---:|---:|---:|---|")
        for r in review:
            why = []
            if r.status == "added":
                why.append("new")
            if r.status == "removed":
                why.append("removed")
            if id(r) in real_dead:
                why.append("no callers")
            if r.after.get("deadCodeCandidate") and id(r) not in real_dead:
                why.append("route/DI call not in graph" if FRAMEWORK_FILE.search(r.file) else "no direct caller")
            if r.status == "removed" and r.before.get("cyclomatic", 0) >= 2:
                why.append(f"was {cell(r.before.get('cyclomatic', 0))} branches")
            if r.status != "removed" and r.d("cyclomatic") >= 2:
                why.append(f"complexity {signed(r.d('cyclomatic'))}")
            if r.status == "added" and r.after.get("loc", 0) >= 40 and r.after.get("cyclomatic", 0) < 5:
                why.append(f"{cell(r.after.get('loc', 0))} lines")
            if abs(r.d("refactorPriority")) >= 5 and r.status in ("changed", "moved"):
                why.append(f"priority {signed(r.d('refactorPriority'), 1)}")
            out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | {arc(*row_metric(r, 'cyclomatic'))} | "
                       f"{arc(*row_metric(r, 'loc'))} | {arc(*row_metric(r, 'callFanIn'))} | "
                       f"{arc(*row_metric(r, 'refactorPriority'), 1)} | {', '.join(why)} |")
    else:
        out.append("_Nothing above the review threshold: only small or mechanical changes._")

    if len(ctx.areas) > 1:
        out.append("\n### By area\n")
        out.append("| area | + / − / ~ | Δcomplexity | Δloc |")
        out.append("|---|---|---:|---:|")
        for a in ctx.areas[:8]:
            out.append(f"| `{a['area']}/` | +{a['added']} / −{a['removed']} / ~{a['changed']} | "
                       f"{signed(a['d_cx'])} | {signed(a['d_loc'])} |")

    tail = [r for r in ctx.ranked if r not in review]
    if tail:
        out.append(f"\n<details><summary>{len(tail)} other symbol changes</summary>\n")
        out.append("")
        out.append("| symbol | file | status | kind | Δcx | Δloc | Δpriority |")
        out.append("|---|---|---|---|---:|---:|---:|")
        for r in tail[:80]:
            out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | {r.kind} | "
                       f"{dcell(*row_metric(r, 'cyclomatic'))} | {dcell(*row_metric(r, 'loc'))} | "
                       f"{dcell(*row_metric(r, 'refactorPriority'), 1)} |")
        out.append("\n</details>")
    if ctx.new_dead:
        fw = len(ctx.new_dead) - len(real_dead)
        out.append(f"\n_Dead-code candidates: {len(real_dead)} without a caller"
                   + (f", {fw} in decorator-registered files (DI/routes are invisible to the call graph)" if fw else "")
                   + "._")
    raw = pd.head_stats["cycles"]
    if raw != len(ctx.cycles_after):
        out.append(f"_Cycles: {raw} in the raw metric, {len(ctx.cycles_after)} after dropping "
                   f"same-name method over-links (LSP call hierarchy links every override of a base method)._")
    if ctx.silent_files:
        out.append(f"_{len(ctx.silent_files)} changed files have no symbol rows "
                   f"(ports, DTOs, schemas, specs): graph sees functions only._")
    return "\n".join(out)


def v_net(pd, ctx):
    """Net effect per area, added minus removed."""
    out = [header(pd)]
    out.append(f"**Net:** {signed(ctx.ta['complexity'] - ctx.tb['complexity'])} cyclomatic, "
               f"{signed(ctx.ta['loc'] - ctx.tb['loc'])} loc, "
               f"{signed(pd.head_stats['nodes'] - pd.base_stats['nodes'])} symbols "
               f"({ctx.by_status.get('added', 0)} added, {ctx.by_status.get('removed', 0)} removed)\n")
    out.append("| area | symbols +/−/~ | net Δcx | net Δloc | new dead (non-framework) |")
    out.append("|---|---|---:|---:|---:|")
    for a in ctx.areas:
        out.append(f"| `{a['area']}/` | +{a['added']} / −{a['removed']} / ~{a['changed']} | "
                   f"{signed(a['d_cx'])} | {signed(a['d_loc'])} | {a['new_dead'] or ''} |")
    return "\n".join(out)


def v_replacements(pd, ctx):
    """Pair removed symbols with the added ones that replace them."""
    out = [header(pd)]
    pairs = []
    for old in ctx.removed:
        candidates = [r for r in ctx.added if r.file == old.file and r.kind == old.kind]
        if candidates:
            new = min(candidates, key=lambda r: abs(r.after.get("cyclomatic", 0) - old.before.get("cyclomatic", 0)))
            pairs.append((old, new))
    if pairs:
        out.append("### Replacement candidates\n")
        out.append("| removed | added | complexity | loc |")
        out.append("|---|---|---|---:|")
        for old, new in pairs:
            out.append(f"| `{old.qname}` | `{new.qname}` | "
                       f"{cell(old.before.get('cyclomatic', 0))} → {cell(new.after.get('cyclomatic', 0))} | "
                       f"{cell(old.before.get('loc', 0))} → {cell(new.after.get('loc', 0))} |")
        out.append("")
    matched_old = {id(o) for o, _ in pairs}
    matched_new = {id(n) for _, n in pairs}
    rest_removed = [r for r in ctx.removed if id(r) not in matched_old]
    rest_added = [r for r in ctx.added if id(r) not in matched_new]
    out.append("### Remaining\n")
    out.append(f"{len(rest_added)} added, {len(rest_removed)} removed, {len(ctx.changed)} changed")
    if ctx.spikes:
        out.append("")
        out.append("Complexity grew in: " + ", ".join(
            f"`{r.qname}` ({signed(r.d('cyclomatic'))})" for r in ctx.spikes[:5]))
    return "\n".join(out)


def v_added_profile(pd, ctx):
    """Profile of the code the PR adds, instead of a row-per-symbol dump."""
    out = [header(pd)]
    prof = ctx.added_profile
    if not prof:
        out.append("_No added symbols: this PR only changes or removes code._")
        return "\n".join(out)
    out.append(f"**{prof['count']} new {plural(prof['count'], 'symbol')}**, {prof['loc_total']} lines total; "
               f"cyclomatic median {prof['cx_median']:.0f}, p90 {prof['cx_p90']:.0f}, max {prof['cx_max']:.0f}; "
               f"{prof['with_callers']} called from existing code.\n")
    big = [r for r in ctx.added if r.after.get("cyclomatic", 0) >= max(5, prof["cx_p90"])]
    if big:
        out.append("| largest new symbols | file | complexity | loc | callers |")
        out.append("|---|---|---:|---:|---:|")
        for r in big[:8]:
            out.append(f"| `{r.qname}` | `{r.file}` | {cell(r.after.get('cyclomatic', 0))} | "
                       f"{cell(r.after.get('loc', 0))} | {cell(r.after.get('callFanIn', 0))} |")
    out.append("")
    if ctx.removed_profile:
        out.append(f"Removed {ctx.removed_profile['count']} {plural(ctx.removed_profile['count'], 'symbol')} "
                   f"({ctx.removed_profile['loc_total']} lines, cyclomatic max {ctx.removed_profile['cx_max']:.0f}).")
    out.append(f"Net cyclomatic {signed(ctx.ta['complexity'] - ctx.tb['complexity'])}, "
               f"net loc {signed(ctx.ta['loc'] - ctx.tb['loc'])} over the whole backend graph.")
    return "\n".join(out)


def v_blind(pd, ctx):
    """What the call graph cannot see: silent files and behaviour-only edits."""
    out = [header(pd)]
    churn = {f["path"].removeprefix("backend/src/"): f for f in pd.files
             if f["path"].startswith("backend/src/")}
    silent = ctx.silent_files
    out.append(f"The graph covers functions and calls. {len(silent)} of {len(pd.changed)} changed files "
               f"produce no symbol rows, so their changes are invisible to this report.\n")
    if silent:
        out.append("| file | lines | why it is silent |")
        out.append("|---|---:|---|")
        for path in silent[:12]:
            info = churn.get(path, {})
            change = f"+{info.get('additions', 0)}/−{info.get('deletions', 0)}"
            if path.endswith(".spec.ts"):
                why = "tests"
            elif ".dto" in path or "schema" in path:
                why = "DTO/schema"
            elif "/ports/" in path:
                why = "port interface"
            elif path.endswith(".module.ts"):
                why = "DI module"
            else:
                why = "no callable symbol delta"
            out.append(f"| `{path}` | {change} | {why} |")
    flat = [r for r in pd.rows if r.d("cyclomatic") == 0 and r.d("callFanIn") == 0]
    if flat:
        out.append("")
        out.append(f"{len(flat)} {plural(len(flat), 'row')} move only in size or priority "
                   f"(behaviour unchanged as far as the graph can tell): "
                   + ", ".join(f"`{r.qname}`" for r in flat[:6]))
    return "\n".join(out)

def dep_block(ctx, limit: int = 6) -> str:
    pairs = cross_pairs(ctx, limit)
    mutual = ctx.mutual_pairs()
    out = []
    if pairs:
        out.append("| from | to | new calls | |")
        out.append("|---|---|---:|---|")
        for (a1, a2), count in pairs:
            flag = "mutual" if tuple(sorted((a1, a2))) in mutual else ""
            out.append(f"| `{a1}/` | `{a2}/` | {count} | {flag} |")
    else:
        out.append("_No new cross-module calls._")

    incoming = ctx.incoming_edges()
    if incoming:
        out.append(f"\n### Existing code that now calls into this PR ({len(incoming)})\n")
        out.append("| existing caller | file | now calls |")
        out.append("|---|---|---|")
        for edge in incoming[:8]:
            out.append(f"| `{edge['src']['qname']}` | `{edge['src']['file']}` | "
                       f"`{edge['dst']['qname']}` |")
        if len(incoming) > 8:
            out.append(f"| _… {len(incoming) - 8} more_ | | |")

    if mutual:
        out.append("")
        out.append("Mutual: " + ", ".join(f"`{a} ↔ {b}`" for a, b in mutual)
                   + ". Pairs through queue handlers are the job-dispatch pattern, not a cycle.")
    hubs = ctx.hubs_gained(6)
    if hubs:
        out.append("")
        out.append("Existing code this PR now depends on: "
                   + ", ".join(f"`{q}` +{c}" for (q, _), c in hubs))
    out.append("")
    out.append(f"{n(ctx.reach())} existing symbols reach the changed files through calls.")
    return "\n".join(out)



def edge_note(edges: dict) -> str:
    real = len(edges["real"])
    total = real + len(edges["spurious"]) + len(edges["test"]) + len(edges["wiring"])
    if total == real:
        return ""
    parts = []
    if edges["dup"]:
        parts.append(f"{len(edges['dup'])} duplicate interface/implementation resolutions")
    if edges["resignature"]:
        parts.append(f"{len(edges['resignature'])} re-resolved after a signature change")
    if edges["unverified"]:
        parts.append(f"{len(edges['unverified'])} resolved to a same-named symbol the line never calls")
    if edges["spurious"]:
        parts.append(f"{len(edges['spurious'])} override artefacts (same method name)")
    if edges["test"]:
        parts.append(f"{len(edges['test'])} test-file edges")
    if edges["wiring"]:
        parts.append(f"{len(edges['wiring'])} DI/module wiring edges")
    return (f"{total} new call edges, {real} used; dropped " + ", ".join(parts))


def cross_pairs(ctx, limit: int = 6):
    return [(k, v) for k, v in ctx.module_pairs().most_common(20) if k[0] != k[1]][:limit]


def v_summary(pd, ctx):
    """Size and cyclomatic next to coupling, reach and call-edge counts."""
    out = [header(pd)]
    b, a = pd.base_stats, pd.head_stats
    tb, ta = ctx.tb, ctx.ta
    rows = [
        ("symbols", n(b["nodes"]), n(a["nodes"]), signed(a["nodes"] - b["nodes"])),
        ("callables", n(b["functions"]), n(a["functions"]), signed(a["functions"] - b["functions"])),
        ("call edges", n(ctx.call_edges("base")), n(ctx.call_edges("head")),
         signed(ctx.call_edges("head") - ctx.call_edges("base"))),
        ("cross-module call edges", n(ctx.cross_module("base")), n(ctx.cross_module("head")),
         signed(ctx.cross_module("head") - ctx.cross_module("base"))),
        ("cyclomatic, total", n(tb["complexity"]), n(ta["complexity"]),
         signed(ta["complexity"] - tb["complexity"])),
        ("cyclomatic per callable",
         f"{tb['complexity'] / max(1, b['functions']):.2f}",
         f"{ta['complexity'] / max(1, a['functions']):.2f}",
         f"{ta['complexity'] / max(1, a['functions']) - tb['complexity'] / max(1, b['functions']):+.2f}"),
        ("loc", n(tb["loc"]), n(ta["loc"]), signed(ta["loc"] - tb["loc"])),
    ]
    out.append("| metric | before | after | Δ |")
    out.append("|---|---:|---:|---:|")
    for label, before, after, delta in rows:
        out.append(f"| {label} | {before} | {after} | {delta} |")
    out.append("")
    prof = ctx.added_profile
    if prof:
        out.append(f"Added {prof['count']} callables ({prof['loc_total']} lines): cyclomatic median "
                   f"{prof['cx_median']:.0f}, p90 {prof['cx_p90']:.0f}, max {prof['cx_max']:.0f}.\n")
    hubs = ctx.hubs_gained(4)
    if hubs:
        out.append("Existing symbols that gained callers: "
                   + ", ".join(f"`{q}` +{c}" for (q, _), c in hubs)
                   + f" · transitive reach {n(ctx.reach())}.")
    note = edge_note(ctx.edges)
    if note:
        out.append(f"\n_{note}._")
    return "\n".join(out)


def v_dep(pd, ctx):
    """What the PR connects that was not connected before."""
    out = [header(pd)]
    pairs = cross_pairs(ctx)
    mutual = ctx.mutual_pairs()
    total = sum(v for k, v in ctx.module_pairs().most_common() if k[0] != k[1])
    out.append(f"{len(ctx.edges['real'])} new call edges, {total} of them across module boundaries.\n")
    if pairs:
        out.append("| from | to | new calls |")
        out.append("|---|---|---:|")
        for (a1, a2), count in pairs:
            out.append(f"| `{a1}/` | `{a2}/` | {count} |")
    if mutual:
        out.append("\nMutual dependencies: " + ", ".join(f"`{a} ↔ {b}`" for a, b in mutual)
                   + ". Pairs that go through queue handlers are the job-dispatch pattern, not a cycle.")
    hubs = ctx.hubs_gained()
    if hubs:
        out.append("\n### Existing code this PR now depends on\n")
        out.append("| symbol | file | new callers |")
        out.append("|---|---|---:|")
        for (qname, file), count in hubs:
            out.append(f"| `{qname}` | `{file}` | +{count} |")
    out.append(f"\n{n(ctx.reach())} existing symbols call into the changed files, directly or transitively.")
    note = edge_note(ctx.edges)
    if note:
        out.append(f"\n_{note}._")
    return "\n".join(out)


def v_arch(pd, ctx):
    """Queues that need a human: dependencies, surface, orphans, checks."""
    out = [header(pd)]
    mutual = ctx.mutual_pairs()
    pairs = cross_pairs(ctx)
    orphans = ctx.orphans()
    exports, gone = ctx.exports()
    hubs = ctx.hubs_gained(5)

    out.append("### Dependency shape\n")
    if pairs:
        out.append("- new cross-module calls: "
                   + ", ".join(f"`{a}/ → {b}/` ×{v}" for (a, b), v in pairs))
    if mutual:
        out.append("- mutual dependencies: " + ", ".join(f"`{a} ↔ {b}`" for a, b in mutual))
    if not pairs and not mutual:
        out.append("- no new cross-module calls")
    if hubs:
        out.append("- existing code that now calls into this PR: "
                   + ", ".join(f"`{q}` +{c}" for (q, _), c in hubs))

    out.append("\n### Surface\n")
    out.append(f"- {len(exports)} new exported symbols"
               + (f"; removed: " + ", ".join(f"`{r.qname}`" for r in gone[:4]) if gone else ""))
    if orphans:
        out.append("- new callables with no callers and no callees: "
                   + ", ".join(f"`{r.qname}`" for r in orphans[:4]))
    out.append(f"- {n(ctx.reach())} existing symbols call into the changed files, directly or transitively")

    out.append("\n### Checks\n")
    real_dead = [r for r in ctx.new_dead if not FRAMEWORK_FILE.search(r.file)]
    checks = [
        f"- dead-code candidates without a caller: {len(real_dead)}"
        + (f" (+{len(ctx.new_dead) - len(real_dead)} in DI/route files, not real)"
           if len(ctx.new_dead) != len(real_dead) else ""),
        f"- cycles: {len(ctx.cycles_after)} real, {pd.head_stats['cycles'] - len(ctx.cycles_after)} dropped as override artefacts",
        f"- {len(ctx.silent_files)} changed files carry no symbol rows (ports, DTOs, schemas, SQL)",
    ]
    out.append("\n".join(checks))
    note = edge_note(ctx.edges)
    if note:
        out.append(f"\n_{note}._")
    return "\n".join(out)


def v_one(pd, ctx):
    """Four bullets: depends on, touches, surface, noise."""
    out = [header(pd)]
    pairs = cross_pairs(ctx, 3)
    mutual = ctx.mutual_pairs()
    hubs = ctx.hubs_gained(3)
    orphans = ctx.orphans()
    exports, gone = ctx.exports()
    prof = ctx.added_profile
    out.append("- **Depends on:** "
               + (", ".join(f"`{a}/ → {b}/` ×{c}" for (a, b), c in pairs) if pairs else "nothing new")
               + (f" · mutual: " + ", ".join(f"`{a} ↔ {b}`" for a, b in mutual) if mutual else ""))
    out.append("- **Touches:** "
               + (", ".join(f"`{q}` +{c}" for (q, _), c in hubs) if hubs else "no existing symbols gained callers")
               + f" · reach {n(ctx.reach())}")
    out.append(f"- **Surface:** {len(exports)} new exported"
               + (f", {len(gone)} removed" if gone else "")
               + (f", orphans: " + ", ".join(f"`{r.qname}`" for r in orphans[:2]) if orphans else ""))
    if prof:
        out.append(f"- **Size:** {prof['count']} callables, {prof['loc_total']} lines, "
                   f"cyclomatic median {prof['cx_median']:.0f} / max {prof['cx_max']:.0f}")
    dropped = len(ctx.edges["spurious"]) + len(ctx.edges["test"]) + len(ctx.edges["wiring"])
    dropped = dropped + len(ctx.edges["dup"])
    out.append(f"- **Noise:** {dropped} of {dropped + len(ctx.edges['real'])} new call edges dropped "
               f"(override/DI/test), {ctx.skipped_rows()} constant/synthetic/test rows, "
               f"{len(ctx.silent_files)} silent files")
    return "\n".join(out)

def v_final2(pd, ctx):
    """General summary, dependency diff, review queue, folded tail."""
    out = [header(pd)]
    b, a = pd.base_stats, pd.head_stats
    tb, ta = ctx.tb, ctx.ta
    out.append(f"`{pd.head_sha}` · +{pd.additions}/−{pd.deletions} lines in {len(pd.changed)} files · "
               f"{ctx.by_status.get('added', 0)} added, {ctx.by_status.get('removed', 0)} removed, "
               f"{ctx.by_status.get('changed', 0)} changed")
    out.append("")

    def line(label, before, after, delta):
        return f"| {label} | {before} | {after} | {delta} |"

    out.append("| metric | before | after | Δ |")
    out.append("|---|---:|---:|---:|")
    out.append(line("symbols", n(b["nodes"]), n(a["nodes"]), signed(a["nodes"] - b["nodes"])))
    out.append(line("callables", n(b["functions"]), n(a["functions"]), signed(a["functions"] - b["functions"])))
    out.append(line("call edges", n(ctx.call_edges("base")), n(ctx.call_edges("head")),
                    signed(ctx.call_edges("head") - ctx.call_edges("base"))))
    out.append(line("cross-module calls", n(ctx.cross_module("base")), n(ctx.cross_module("head")),
                    signed(ctx.cross_module("head") - ctx.cross_module("base"))))
    out.append(line("cyclomatic, total", n(tb["complexity"]), n(ta["complexity"]),
                    signed(ta["complexity"] - tb["complexity"])))
    per_b = tb["complexity"] / max(1, b["functions"])
    per_a = ta["complexity"] / max(1, a["functions"])
    per_d = per_a - per_b
    out.append(line("cyclomatic per callable", f"{per_b:.2f}", f"{per_a:.2f}",
                    f"{per_d:+.2f}" if abs(per_d) >= 0.005 else "0.00"))
    out.append(line("loc", n(tb["loc"]), n(ta["loc"]), signed(ta["loc"] - tb["loc"])))
    out.append("")

    prof = ctx.added_profile
    if prof:
        out.append(f"Added {prof['count']} callables ({prof['loc_total']} lines): cyclomatic median "
                   f"{prof['cx_median']:.0f}, p90 {prof['cx_p90']:.0f}, max {prof['cx_max']:.0f}; "
                   f"{prof['with_callers']} are called from existing code.")

    out.append("\n### Dependency diff\n")
    out.append(dep_block(ctx))

    real_dead = {id(r) for r in ctx.new_dead if not FRAMEWORK_FILE.search(r.file)}
    skipped = []
    notable_added, notable_changed, notable_removed = [], [], []
    for r in ctx.ranked:
        if r.kind == "constant" or r.synthetic or r.file.endswith(".spec.ts"):
            skipped.append(r)
            continue
        if r.status == "removed":
            notable_removed.append(r)
        elif r.status == "added":
            notable_added.append(r)
        elif abs(r.d("cyclomatic")) >= 1 or abs(r.d("refactorPriority")) >= 5:
            notable_changed.append(r)
    notable_added.sort(key=lambda r: (r.after.get("cyclomatic", 0), r.after.get("loc", 0)), reverse=True)
    review = notable_removed[:3] + notable_added[:6] + notable_changed[:5]
    seen = set()
    review = [r for r in review if not (id(r) in seen or seen.add(id(r)))][:12]

    out.append(f"\n### Review these ({len(review)} of {len(pd.rows)})\n")
    if review:
        out.append("| symbol | file | status | complexity | loc | callers | priority | why |")
        out.append("|---|---|---|---:|---:|---:|---:|---|")
        for r in review:
            why = []
            if r.status == "added":
                why.append("new")
            if r.status == "removed":
                why.append("removed")
            if id(r) in real_dead:
                why.append("no callers")
            if r.after.get("deadCodeCandidate") and id(r) not in real_dead:
                why.append("route/DI call not in graph" if FRAMEWORK_FILE.search(r.file) else "no direct caller")
            if r.status == "removed" and r.before.get("cyclomatic", 0) >= 2:
                why.append(f"was {cell(r.before.get('cyclomatic', 0))} branches")
            if r.status != "removed" and r.d("cyclomatic") >= 2:
                why.append(f"complexity {signed(r.d('cyclomatic'))}")
            if r.status == "added" and r.after.get("loc", 0) >= 40 and r.after.get("cyclomatic", 0) < 5:
                why.append(f"{cell(r.after.get('loc', 0))} lines")
            if abs(r.d("refactorPriority")) >= 5 and r.status in ("changed", "moved"):
                why.append(f"priority {signed(r.d('refactorPriority'), 1)}")
            out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | {arc(*row_metric(r, 'cyclomatic'))} | "
                       f"{arc(*row_metric(r, 'loc'))} | {arc(*row_metric(r, 'callFanIn'))} | "
                       f"{arc(*row_metric(r, 'refactorPriority'), 1)} | {', '.join(why)} |")
    else:
        out.append("_Nothing above the review threshold: only small or mechanical changes._")
    if skipped:
        out.append(f"\n{len(skipped)} constants, synthetic module constructors and test-file rows left out.")

    if len(ctx.areas) > 1:
        out.append("\n### By area\n")
        out.append("| area | + / − / ~ | Δcyclomatic | Δloc |")
        out.append("|---|---|---:|---:|")
        for area in ctx.areas[:8]:
            label = area["area"] + ("" if "." in area["area"] else "/")
            out.append(f"| `{label}` | +{area['added']} / −{area['removed']} / ~{area['changed']} | "
                       f"{signed(area['d_cx'])} | {signed(area['d_loc'])} |")

    tail = [r for r in ctx.ranked if r not in review]
    if tail:
        out.append(f"\n<details><summary>{len(tail)} other symbol changes</summary>\n")
        out.append("")
        out.append("| symbol | file | status | kind | Δcyclomatic | Δloc | Δpriority |")
        out.append("|---|---|---|---|---:|---:|---:|")
        for r in tail[:80]:
            out.append(f"| `{r.qname}` | `{r.file}` | {r.status} | {r.kind} | "
                       f"{dcell(*row_metric(r, 'cyclomatic'))} | {dcell(*row_metric(r, 'loc'))} | "
                       f"{dcell(*row_metric(r, 'refactorPriority'), 1)} |")
        if len(tail) > 80:
            out.append(f"| … {len(tail) - 80} more | | | | | | |")
        out.append("\n</details>")

    note = edge_note(ctx.edges)
    if note:
        out.append(f"\n_{note}._")
    raw = pd.head_stats["cycles"]
    if raw != len(ctx.cycles_after):
        out.append(f"_Cycles: {raw} in the raw metric, {len(ctx.cycles_after)} real "
                   f"(same-name override links dropped)._")
    if ctx.new_dead:
        fw = len(ctx.new_dead) - len(real_dead)
        out.append(f"_Dead-code candidates: {len(real_dead)} without a caller"
                   + (f", {fw} in DI/route files where decorators are invisible to the graph" if fw else "")
                   + "._")
    if ctx.silent_files:
        out.append(f"_{len(ctx.silent_files)} changed files carry no symbol rows (ports, DTOs, schemas, SQL)._")
    return "\n".join(out)


def v_final3(pd, ctx):
    """Who now depends on what: load-bearing existing symbols first."""
    out = [header(pd)]
    b, a = pd.base_stats, pd.head_stats
    tb, ta = ctx.tb, ctx.ta
    per_b = tb["complexity"] / max(1, b["functions"])
    per_a = ta["complexity"] / max(1, a["functions"])
    prof = ctx.added_profile
    d_nodes = a["nodes"] - b["nodes"]
    d_callables = a["functions"] - b["functions"]
    d_edges = ctx.call_edges("head") - ctx.call_edges("base")
    if d_nodes == d_callables == d_edges == 0:
        shape = "graph shape unchanged: no new symbols, callables or call edges"
    else:
        shape = (f"{signed(d_nodes)} symbols, {signed(d_callables)} callables, "
                 f"{signed(d_edges)} call edges "
                 f"({signed(ctx.cross_module('head') - ctx.cross_module('base'))} across modules)")
    out.append(f"`{pd.head_sha}` · +{pd.additions}/−{pd.deletions} lines in {len(pd.changed)} files · {shape}")
    out.append(f"{n(ctx.reach())} existing symbols reach the changed files · "
               f"cyclomatic/callable {per_b:.2f} → {per_a:.2f} · loc {signed(ta['loc'] - tb['loc'])}")
    if prof:
        out.append(f"\nAdded {prof['count']} callables ({prof['loc_total']} lines, cyclomatic median "
                   f"{prof['cx_median']:.0f} / max {prof['cx_max']:.0f}); "
                   f"{prof['with_callers']} are called from existing code.")

    hubs = ctx.hubs_gained(10)
    out.append("\n### Existing code this PR now depends on\n")
    out.append("Existing symbols the new code starts calling.\n")
    if hubs:
        out.append("| existing symbol | new calls into it |")
        out.append("|---|---:|")
        for (qname, _file), count in hubs:
            out.append(f"| `{qname}` | +{count} |")
    else:
        out.append("The PR calls only its own new code.")
    out.append("")
    out.append("<details><summary>Columns and what this measures</summary>")
    out.append("")
    out.append("- **new calls into it**: how many call sites the PR added into that existing symbol.")
    out.append("- A high count means the feature leans on it; if it is a port or shared helper, the PR deepens a central dependency.")
    out.append("- Only new call edges from the PR are counted: overrides, DI wiring, tests, duplicates and re-resolutions are dropped.")
    out.append("")
    out.append("</details>")

    out.append("\n### What this PR depends on\n")
    out.append("New call edges between modules, aggregated.\n")
    pairs = cross_pairs(ctx)
    mutual = ctx.mutual_pairs()
    if pairs:
        out.append("| from | to | new calls |")
        out.append("|---|---|---:|")
        for (a1, a2), count in pairs:
            out.append(f"| `{a1}/` | `{a2}/` | {count} |")
        if mutual:
            out.append("")
            out.append("Mutual: " + ", ".join(f"`{a} ↔ {b}`" for a, b in mutual)
                       + ". Pairs through queue handlers are the job-dispatch pattern, not a cycle.")
    else:
        out.append("No new cross-module calls.")
    out.append("")
    out.append("<details><summary>Columns and what this measures</summary>")
    out.append("")
    out.append("- **from / to**: module, the first path segment of the file.")
    out.append("- **new calls**: real new call edges after dropping overrides, DI wiring, tests, duplicates and re-resolutions.")
    out.append("- **mutual**: both directions gained edges. Normal for queue/dispatch patterns, a warning for pure logic modules.")
    out.append("")
    out.append("</details>")

    incoming = ctx.incoming_edges()
    if incoming:
        out.append(f"\n### Existing code that now calls into this PR ({len(incoming)})\n")
        out.append("Unchanged files that start calling the touched symbols.\n")
        out.append("| existing caller | now calls |")
        out.append("|---|---|")
        for edge in incoming[:8]:
            out.append(f"| `{edge['src']['qname']}` | `{edge['dst']['qname']}` |")
        if len(incoming) > 8:
            out.append(f"| _… {len(incoming) - 8} more_ | | |")

    affected_total, affected_changed, affected_added, affected_direct, affected_rows = ctx.affected_existing(8)
    out.append(f"\n### Existing code affected transitively ({affected_total})\n")
    if affected_added:
        out.append(f"{affected_changed} depend on modified code, {affected_added} reach only symbols the PR added.\n")
    out.append("Unchanged callables whose execution path runs through symbols the PR touched.\n")
    if affected_rows:
        if affected_direct:
            out.append(f"{affected_direct} of them call a touched symbol directly; the rest are one or more calls away.\n")
        else:
            out.append("All of them are one or more calls away from the touched symbols.\n")
        out.append("| existing symbol | changed | added | share | priority | direct |")
        out.append("|---|---:|---:|---:|---:|---|")
        for item in affected_rows:
            share = 100 * item["changed"] / max(1, item["all_callees"])
            out.append(f"| `{item['qname']}` | {item['changed']} | {item['added'] or ''} | {share:.0f}% | "
                       f"{n(item['priority'])} | {'yes' if item['direct'] else ''} |")
        out.append("")
        out.append("<details><summary>Columns and what this measures</summary>")
        out.append("")
        out.append("- **changed**: touched symbols whose code the PR modified and that this callable can reach.")
        out.append("- **added**: touched symbols the PR created and that it can reach too. Additions alone do not change behaviour; they are listed separately.")
        out.append("- **share**: changed / all reachable callees. How much of the function's execution surface the PR rewrote.")
        out.append("- **priority**: refactor priority of the unchanged function. High share with high priority means read it first.")
        out.append("- **direct**: it calls a touched symbol itself, not through an intermediate.")
        out.append("- Call edges only: shared config, DTO and constant data flow is not counted, so this is a lower bound.")
        out.append("")
        out.append("</details>")
    else:
        out.append("No unchanged callable depends on the touched symbols: the change stays inside the files it edits.")

    real_dead = {id(r) for r in ctx.new_dead if not FRAMEWORK_FILE.search(r.file)}
    skipped = []
    notable_added, notable_changed, notable_removed = [], [], []
    for r in ctx.ranked:
        if r.kind == "constant" or r.synthetic or r.file.endswith(".spec.ts"):
            skipped.append(r)
            continue
        if r.status == "removed":
            notable_removed.append(r)
        elif r.status == "added":
            notable_added.append(r)
        elif abs(r.d("cyclomatic")) >= 1 or abs(r.d("refactorPriority")) >= 5:
            notable_changed.append(r)
    notable_added.sort(key=lambda r: (r.after.get("cyclomatic", 0), r.after.get("loc", 0)), reverse=True)
    review = notable_removed[:3] + notable_added[:6] + notable_changed[:5]
    seen = set()
    review = [r for r in review if not (id(r) in seen or seen.add(id(r)))][:12]

    out.append(f"\n### Review these ({len(review)} of {len(pd.rows)})\n")
    out.append("Symbols worth reading first.\n")
    if review:
        out.append("| symbol | status | complexity | loc | callers | priority | why |")
        out.append("|---|---|---:|---:|---:|---:|---|")
        for r in review:
            why = []
            if r.status == "added":
                why.append("new")
            if r.status == "removed":
                why.append("removed")
            if id(r) in real_dead:
                why.append("no callers")
            if r.after.get("deadCodeCandidate") and id(r) not in real_dead:
                why.append("route/DI call not in graph" if FRAMEWORK_FILE.search(r.file) else "no direct caller")
            if r.status == "removed" and r.before.get("cyclomatic", 0) >= 2:
                why.append(f"was {cell(r.before.get('cyclomatic', 0))} branches")
            if r.status != "removed" and r.d("cyclomatic") >= 2:
                why.append(f"complexity {signed(r.d('cyclomatic'))}")
            if r.status == "added" and r.after.get("loc", 0) >= 40 and r.after.get("cyclomatic", 0) < 5:
                why.append(f"{cell(r.after.get('loc', 0))} lines")
            if abs(r.d("refactorPriority")) >= 5 and r.status in ("changed", "moved"):
                why.append(f"priority {signed(r.d('refactorPriority'), 1)}")
            out.append(f"| `{r.qname}` | {r.status} | {arc(*row_metric(r, 'cyclomatic'))} | "
                       f"{arc(*row_metric(r, 'loc'))} | {arc(*row_metric(r, 'callFanIn'))} | "
                       f"{arc(*row_metric(r, 'refactorPriority'), 1)} | {', '.join(why)} |")
    else:
        out.append("Nothing above the review threshold: only small or mechanical changes.")
    if skipped:
        out.append(f"\n{len(skipped)} constants, synthetic module constructors and test-file rows left out.")
    out.append("")
    out.append("<details><summary>Columns and what this measures</summary>")
    out.append("")
    out.append("- **status**: added, changed or removed. Added and removed rows show signed metric values instead of a delta.")
    out.append("- **complexity / loc / callers**: cyclomatic, lines and incoming calls; added and removed rows show `+N` or `−N`.")
    out.append("- **priority**: refactor priority; `A → B` for changed symbols, `+N` for new ones.")
    out.append("- **why**: why the row is in the list — size, complexity jump, removal, missing caller.")
    out.append("")
    out.append("</details>")

    if len(ctx.areas) > 1:
        out.append("\n### By area\n")
        out.append("Where the change lands.\n")
        out.append("| area | + / − / ~ | cyclomatic/callable | Δloc |")
        out.append("|---|---|---:|---:|")
        for area in ctx.areas[:6]:
            label = area["area"] + ("" if "." in area["area"] else "/")
            out.append(f"| `{label}` | +{area['added']} / −{area['removed']} / ~{area['changed']} | "
                       f"{ctx.area_avg_cx(area['area'])} | {signed(area['d_loc'])} |")
        if len(ctx.areas) > 6:
            out.append(f"| … {len(ctx.areas) - 6} more areas | | | |")
        out.append("")
        out.append("<details><summary>Columns and what this measures</summary>")
        out.append("")
        out.append("- **+ / − / ~**: symbols added, removed and changed in that area.")
        out.append("- **cyclomatic/callable**: mean cyclomatic of the area's callables; a `→` appears only when it moved. Absolute sums grow with file count and say little.")
        out.append("- **Δloc**: absolute line delta; fine here because it tracks effort, not complexity.")
        out.append("")
        out.append("</details>")

    tail = [r for r in ctx.ranked if r not in review]
    if tail:
        out.append(f"\n<details><summary>{len(tail)} other symbol changes</summary>\n")
        out.append("")
        out.append("| symbol | status | kind | Δcyclomatic | Δloc | Δpriority |")
        out.append("|---|---|---|---:|---:|---:|")
        for r in tail[:80]:
            out.append(f"| `{r.qname}` | {r.status} | {r.kind} | "
                       f"{dcell(*row_metric(r, 'cyclomatic'))} | {dcell(*row_metric(r, 'loc'))} | "
                       f"{dcell(*row_metric(r, 'refactorPriority'), 1)} |")
        if len(tail) > 80:
            out.append(f"| … {len(tail) - 80} more | | | | | |")
        out.append("\n</details>")

    note = edge_note(ctx.edges)
    if note:
        out.append(f"\n{note}.")
    raw = pd.head_stats["cycles"]
    if raw != len(ctx.cycles_after):
        out.append(f"Cycles: {raw} raw, {len(ctx.cycles_after)} real (same-name override links dropped).")
    if ctx.new_dead:
        fw = len(ctx.new_dead) - len(real_dead)
        out.append(f"Dead-code candidates: {len(real_dead)} without a caller"
                   + (f", {fw} in DI/route files where decorators are invisible to the graph" if fw else "")
                   + ".")
    if ctx.silent_files:
        out.append(f"{len(ctx.silent_files)} changed files carry no symbol rows (ports, DTOs, schemas, SQL).")
    return "\n".join(out)



VARIANTS = [
    ("final3", "Final: dependency-first", v_final3),
]


# --------------------------------------------------------------------------- #
# page
# --------------------------------------------------------------------------- #

TEMPLATE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>code-graph PR report variants</title>
<style>
:root { color-scheme: light dark; --fg: #1f2328; --muted: #656d76; --bg: #fff;
  --surface: #f6f8fa; --border: #d1d9e0; --accent: #0969da; --warn: #9a6700; }
@media (prefers-color-scheme: dark) {
  :root { --fg: #e6edf3; --muted: #9198a1; --bg: #0d1117; --surface: #161b22;
    --border: #30363d; --accent: #4493f8; --warn: #d29922; }
}
* { box-sizing: border-box; }
body { margin: 0; font: 15px/1.5 -apple-system, "Segoe UI", Roboto, sans-serif;
  color: var(--fg); background: var(--bg); }
header { padding: 10px 20px; border-bottom: 1px solid var(--border);
  display: flex; gap: 14px; align-items: center; flex-wrap: wrap; position: sticky; top: 0;
  background: var(--bg); z-index: 5; }
header select { font: inherit; padding: 4px 8px; background: var(--surface);
  color: var(--fg); border: 1px solid var(--border); border-radius: 6px; max-width: 560px; }
h1 { font-size: 17px; margin: 0; white-space: nowrap; }
.layout { display: grid; grid-template-columns: 280px minmax(0, 1fr); }
nav { border-right: 1px solid var(--border); padding: 10px; align-self: start;
  position: sticky; top: 51px; max-height: calc(100vh - 51px); overflow: auto; }
nav button { display: block; width: 100%; text-align: left; margin: 0 0 5px; padding: 7px 9px;
  border: 1px solid var(--border); border-radius: 6px; background: var(--surface);
  color: var(--fg); font: inherit; cursor: pointer; }
nav button small { display: block; color: var(--muted); font-size: 11.5px; }
nav button.active { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
main { padding: 16px 24px 90px; }
.meta { color: var(--muted); font-size: 13px; margin-bottom: 10px; }
.meta b { color: var(--fg); font-weight: 600; }
#panes { display: grid; gap: 16px; align-items: start; }
#panes.two { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
.pane .label { color: var(--accent); font-size: 12px; font-weight: 600; margin-bottom: 6px; }
.comment { border: 1px solid var(--border); border-radius: 8px; padding: 14px 18px; overflow-x: auto; }
.comment h2 { font-size: 1.25em; margin: 0 0 10px; border-bottom: 1px solid var(--border); padding-bottom: 6px; }
.comment h3 { font-size: 1.08em; margin: 14px 0 8px; }
.md-table { border-collapse: collapse; margin: 8px 0 14px; font-size: 12.5px; }
.md-table th, .md-table td { border: 1px solid var(--border); padding: 4px 8px; text-align: left; }
.md-table th { background: var(--surface); }
.md-table td { white-space: nowrap; max-width: 420px; overflow: hidden; text-overflow: ellipsis; }
.comment code { background: var(--surface); padding: 1px 4px; border-radius: 4px; font-size: 12px; }
.comment pre.code { background: var(--surface); border-radius: 6px; padding: 10px; overflow: auto; white-space: pre-wrap; }
details { margin: 8px 0; }
summary { cursor: pointer; color: var(--accent); }
.fail { border-left: 3px solid var(--warn); padding: 8px 12px; background: var(--surface);
  margin: 0 0 12px; font-size: 13px; }
.fail ul { margin: 6px 0 0; padding-left: 20px; }
.toolbar { margin: 12px 0; }
.toolbar button { font: inherit; padding: 5px 10px; border: 1px solid var(--border);
  border-radius: 6px; background: var(--surface); color: var(--fg); cursor: pointer; }
</style>
</head>
<body>
<header>
  <h1>code-graph report variants</h1>
  <select id="pr"></select>
  <select id="cmp"><option value="">compare: none</option></select>
  <span class="meta" id="prmeta"></span>
</header>
<div class="layout">
  <nav id="variants"></nav>
  <main>
    <div class="fail" id="diagnosis"></div>
    <div class="meta" id="variantmeta"></div>
    <div id="panes">
      <div class="pane" id="pane-a" hidden><div class="label" id="label-a"></div><div class="comment" id="comment-a"></div></div>
      <div class="pane" id="pane-b"><div class="label" id="label-b"></div><div class="comment" id="comment-b"></div></div>
    </div>
    <div class="toolbar"><button id="copy">copy markdown</button></div>
    <details class="raw"><summary>markdown source</summary><pre class="code" id="rawmd"></pre></details>
    <details class="raw"><summary>raw diff rows (audit diff JSON)</summary><pre class="code" id="rawrows"></pre></details>
  </main>
</div>
<script>
const DATA = __DATA__;
const prSel = document.getElementById('pr');
const cmpSel = document.getElementById('cmp');
const nav = document.getElementById('variants');
const panes = document.getElementById('panes');
let currentVariant = DATA.variants[0].id;

DATA.prs.forEach(pr => {
  const opt = document.createElement('option');
  opt.value = pr.number;
  opt.textContent = '#' + pr.number + ' · ' + pr.title;
  prSel.appendChild(opt);
});

DATA.variants.forEach(v => {
  {
    const opt = document.createElement('option');
    opt.value = v.id;
    opt.textContent = 'compare: ' + v.name;
    cmpSel.appendChild(opt);
  }
  const b = document.createElement('button');
  b.dataset.id = v.id;
  b.innerHTML = v.name + '<small>' + v.note + '</small>';
  b.onclick = () => { currentVariant = v.id; paint(); };
  nav.appendChild(b);
});

const byId = id => DATA.variants.find(v => v.id === id);

function paint() {
  const pr = DATA.prs.find(p => String(p.number) === prSel.value);
  document.getElementById('prmeta').innerHTML =
    '<b>+' + pr.additions + ' / -' + pr.deletions + '</b> lines · ' + pr.files + ' files · ' + pr.status;
  const diag = document.getElementById('diagnosis');
  diag.innerHTML = '<b>What this PR exposes</b><ul>' +
    pr.diagnosis.map(d => '<li>' + d + '</li>').join('') + '</ul>';

  const cmpId = cmpSel.value;
  document.getElementById('comment-b').innerHTML = pr.renders[currentVariant];
  document.getElementById('label-b').textContent = byId(currentVariant).name;
  if (cmpId) {
    panes.classList.add('two');
    document.getElementById('pane-a').hidden = false;
    document.getElementById('comment-a').innerHTML = pr.renders[cmpId];
    document.getElementById('label-a').textContent = byId(cmpId).name;
  } else {
    panes.classList.remove('two');
    document.getElementById('pane-a').hidden = true;
  }
  const v = byId(currentVariant);
  document.getElementById('variantmeta').innerHTML = '<b>' + v.name + '</b> — ' + v.note;
  document.getElementById('rawmd').textContent = pr.markdown[currentVariant];
  document.getElementById('rawrows').textContent = pr.raw;
  nav.querySelectorAll('button').forEach(b => b.classList.toggle('active', b.dataset.id === currentVariant));
}

prSel.value = DATA.prs[0].number;
prSel.onchange = paint;
cmpSel.onchange = paint;
document.addEventListener('keydown', e => {
  const ids = DATA.variants.map(v => v.id);
  const at = ids.indexOf(currentVariant);
  if (e.key === 'ArrowRight') { currentVariant = ids[Math.min(at + 1, ids.length - 1)]; paint(); }
  if (e.key === 'ArrowLeft') { currentVariant = ids[Math.max(at - 1, 0)]; paint(); }
});
document.getElementById('copy').onclick = async () => {
  await navigator.clipboard.writeText(document.getElementById('rawmd').textContent);
};
paint();
</script>
</body>
</html>
"""


def build(pr_numbers: list[int]) -> None:
    prs = []
    for number in pr_numbers:
        pd = lib.load(number)
        ctx = Ctx(pd)
        renders, markdown, failures = {}, {}, {}
        for vid, vname, fn in VARIANTS:
            try:
                body = fn(pd, ctx)
            except Exception as exc:  # a broken variant must not kill the page
                body = f"{header(pd)}\n_variant failed: {exc}_"
            markdown[vid] = body + "\n\n<!-- meta: snapshots differ; see page -->"
            renders[vid] = md.render(body)
            failures[vid] = ""
        prs.append({
            "number": pd.number,
            "title": pd.title,
            "additions": pd.additions,
            "deletions": pd.deletions,
            "files": len(pd.changed),
            "status": status_counts(ctx),
            "diagnosis": ctx.diagnosis,
            "raw": json.dumps([{
                "qname": r.qname, "file": r.file, "status": r.status,
                "before": {k: round(v, 4) if isinstance(v, float) else v for k, v in r.before.items()},
                "after": {k: round(v, 4) if isinstance(v, float) else v for k, v in r.after.items()},
            } for r in pd.rows], indent=1),
            "renders": renders,
            "markdown": markdown,
            "failures": failures,
        })
        print(f"#{pd.number}: {len(pd.rows)} rows, {status_counts(ctx)}", file=sys.stderr)

    data = {
        "prs": prs,
        "variants": [{"id": vid, "name": vname, "note": (fn.__doc__ or "").strip().split("\n")[0]}
                     for vid, vname, fn in VARIANTS],
    }
    html = TEMPLATE.replace("__DATA__", json.dumps(data, ensure_ascii=False))
    with open(OUT, "w") as fh:
        fh.write(html)
    print(f"wrote {OUT} ({len(html)} bytes)")


if __name__ == "__main__":
    numbers = [int(a) for a in sys.argv[1:]] or sorted(
        int(name) for name in os.listdir(lib.DATA)
        if name.isdigit() and os.path.exists(f"{lib.DATA}/{name}/diff.json"))
    build(numbers)
