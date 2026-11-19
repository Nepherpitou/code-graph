#!/usr/bin/env python3
"""Load code-graph snapshots for a PR and derive report-ready data."""
from __future__ import annotations

import json
import os
from collections import defaultdict
from dataclasses import dataclass, field

DATA = os.environ.get("CGPR_DIR", "/tmp/cgpr")

# Metrics worth showing per symbol, in display order.
SYMBOL_METRICS = [
    "refactorPriority",
    "cyclomatic",
    "loc",
    "sloc",
    "params",
    "callFanIn",
    "callFanOut",
    "ifc",
    "transitiveFanIn",
    "moduleRefs",
    "hotspot",
]


@dataclass
class Row:
    qname: str
    file: str
    kind: str
    status: str
    line: int | None
    exported: bool
    synthetic: bool
    before: dict
    after: dict
    flags: list[str]

    @property
    def delta(self) -> dict:
        keys = set(self.before) | set(self.after)
        return {k: self.after.get(k, 0) - self.before.get(k, 0) for k in keys}

    def d(self, key: str) -> float:
        return self.after.get(key, 0) - self.before.get(key, 0)


@dataclass
class PRData:
    number: int
    title: str
    body: str
    additions: int
    deletions: int
    base_sha: str
    head_sha: str
    files: list[dict]
    base_stats: dict
    head_stats: dict
    rows: list[Row]
    total_rows: int
    base_nodes: dict
    head_nodes: dict
    base_edges: list
    head_edges: list
    changed: list[str]


def _metrics(node: dict) -> dict:
    m = dict(node.get("metrics") or {})
    m["cyclomatic"] = node.get("cyclomatic", 0)
    m["loc"] = node.get("loc", 0)
    m["sloc"] = node.get("sloc", 0)
    m["params"] = node.get("params", 0)
    return m


def load(n: int) -> PRData:
    d = f"{DATA}/{n}"
    base = json.load(open(f"{d}/base.json"))
    head = json.load(open(f"{d}/head.json"))
    diff = json.load(open(f"{d}/diff.json"))
    meta = json.load(open(f"{DATA}/meta/{n}.json"))
    changed = [line.strip() for line in open(f"{d}/changed.txt") if line.strip()]

    base_nodes = {node["id"]: node for node in base["nodes"]}
    head_nodes = {node["id"]: node for node in head["nodes"]}

    rows = []
    for raw in diff["rows"]:
        before = _metrics(base_nodes[raw["fromId"]]) if raw.get("fromId") in base_nodes else {}
        after = _metrics(head_nodes[raw["toId"]]) if raw.get("toId") in head_nodes else {}
        node = head_nodes.get(raw.get("toId")) or base_nodes.get(raw.get("fromId")) or {}
        rows.append(
            Row(
                qname=raw.get("qname") or node.get("qname") or raw.get("id", ""),
                file=raw.get("file", node.get("file", "")),
                kind=node.get("kind", "?"),
                status=raw["status"],
                line=node.get("line"),
                exported=bool(node.get("exported")),
                synthetic=bool(node.get("synthetic")),
                before=before,
                after=after,
                flags=list(raw.get("flags") or []),
            )
        )

    return PRData(
        number=meta["number"],
        title=meta["title"],
        body=meta.get("body") or "",
        additions=meta.get("additions", 0),
        deletions=meta.get("deletions", 0),
        base_sha=(meta.get("baseRefOid") or "")[:8],
        head_sha=(meta.get("headRefOid") or "")[:8],
        files=meta.get("files") or [],
        base_stats=base["stats"],
        head_stats=head["stats"],
        rows=rows,
        total_rows=diff.get("totalRows", len(rows)),
        base_nodes=base_nodes,
        head_nodes=head_nodes,
        base_edges=base["edges"],
        head_edges=head["edges"],
        changed=changed,
    )


def totals(nodes: dict) -> dict:
    """Graph-wide absolute totals that a diff can compare."""
    def add(acc, node):
        acc["complexity"] += node.get("cyclomatic", 0)
        acc["loc"] += node.get("loc", 0)
        acc["sloc"] += node.get("sloc", 0)
        acc["priority"] += (node.get("metrics") or {}).get("refactorPriority", 0.0)
        acc["ifc"] += (node.get("metrics") or {}).get("ifc", 0.0)

    acc = defaultdict(float)
    for node in nodes.values():
        add(acc, node)
    return dict(acc)


def file_rollup(pd: PRData) -> list[dict]:
    """Per-file symbol churn and complexity delta."""
    groups = defaultdict(lambda: {"added": 0, "removed": 0, "changed": 0, "moved": 0,
                                  "d_complexity": 0, "d_loc": 0, "d_priority": 0.0,
                                  "new_dead": 0, "max_d_priority": 0.0})
    for row in pd.rows:
        g = groups[row.file]
        if row.status in ("added", "removed", "changed", "moved"):
            g[row.status] += 1
        g["d_complexity"] += row.d("cyclomatic")
        g["d_loc"] += row.d("loc")
        g["d_priority"] += row.d("refactorPriority")
        g["max_d_priority"] = max(g["max_d_priority"], abs(row.d("refactorPriority")))
        if row.status == "added" and row.after.get("deadCodeCandidate"):
            g["new_dead"] += 1
    out = []
    for path, g in groups.items():
        out.append({"file": path, "added": g["added"], "removed": g["removed"],
                    "changed": g["changed"], "d_complexity": g["d_complexity"],
                    "d_loc": g["d_loc"], "d_priority": g["d_priority"],
                    "new_dead": g["new_dead"], "max_d_priority": g["max_d_priority"]})
    out.sort(key=lambda r: (abs(r["d_complexity"]), abs(r["d_priority"])), reverse=True)
    return out


def cycles_of(nodes: dict, edges: list[dict]) -> list[list[str]]:
    """Tarjan SCCs over call+data edges, cycles only."""
    adj = defaultdict(list)
    for edge in edges:
        if edge["source"] in nodes and edge["target"] in nodes:
            adj[edge["source"]].append(edge["target"])

    import sys
    sys.setrecursionlimit(max(10000, len(nodes) * 3))

    index = {}
    low = {}
    stack = []
    on_stack = set()
    counter = [0]
    result = []

    def strongconnect(v):
        index[v] = low[v] = counter[0]
        counter[0] += 1
        stack.append(v)
        on_stack.add(v)
        for w in adj.get(v, ()):
            if w not in index:
                strongconnect(w)
                low[v] = min(low[v], low[w])
            elif w in on_stack:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            comp = []
            while True:
                w = stack.pop()
                on_stack.discard(w)
                comp.append(w)
                if w == v:
                    break
            if len(comp) > 1:
                result.append(comp)

    for v in nodes:
        if v not in index:
            strongconnect(v)
    return result


def new_dead_symbols(pd: PRData) -> list[Row]:
    """Added symbols flagged as dead-code candidates."""
    return [r for r in pd.rows if r.status == "added" and r.after.get("deadCodeCandidate")]


def boundary_rows(pd: PRData) -> list[Row]:
    """Changed or added symbols that callers outside the diff can reach."""
    return [r for r in pd.rows if r.after.get("callFanIn", 0) > 0 or r.before.get("callFanIn", 0) > 0]


def summarize(pd: PRData) -> str:
    out = []
    out.append(f"PR #{pd.number} {pd.title}  (+{pd.additions}/-{pd.deletions}, {len(pd.changed)} backend files)")
    for side, stats in (("base", pd.base_stats), ("head", pd.head_stats)):
        out.append(f"  {side}: nodes={stats['nodes']} edges={stats['edges']} cycles={stats['cycles']} "
                   f"dead={stats['deadCode']} crossFile={stats['crossFileRatio']:.3f}")
    bt, ht = totals(pd.base_nodes), totals(pd.head_nodes)
    out.append(f"  totals: complexity {bt['complexity']:.0f} -> {ht['complexity']:.0f}, "
               f"loc {bt['loc']:.0f} -> {ht['loc']:.0f}, priority {bt['priority']:.1f} -> {ht['priority']:.1f}")
    by_status = defaultdict(int)
    for row in pd.rows:
        by_status[row.status] += 1
    out.append(f"  rows: total={pd.total_rows} {dict(by_status)}")
    out.append(f"  new dead symbols: {len(new_dead_symbols(pd))}, boundary symbols: {len(boundary_rows(pd))}")
    for r in file_rollup(pd)[:6]:
        out.append(f"    {r['file']}: +{r['added']} -{r['removed']} ~{r['changed']} "
                   f"dCx={r['d_complexity']:+d} dLoc={r['d_loc']:+d} newDead={r['new_dead']}")
    return "\n".join(out)


if __name__ == "__main__":
    import sys
    for n in sys.argv[1:]:
        pd = load(int(n))
        print(summarize(pd))
        print()
