#!/usr/bin/env python3
"""Render a pull-request comment from code-graph audit-diff views.

The tool prints one view per call; this script is the only place that knows how
the comment looks. Views are read from a directory of JSON files:

    summary.json uses.json dependencies.json callers.json affected.json
    review.json symbols.json areas.json filtered.json silent.json dead.json

Missing files are treated as empty views; a missing summary means the analysis
did not run.
"""

from __future__ import annotations

import argparse
import json
import os
import sys

FRAMEWORK_SUFFIXES = (
    ".controller.ts",
    ".module.ts",
    ".provider.ts",
    ".guard.ts",
    ".interceptor.ts",
)


def load(directory: str, name: str) -> dict:
    path = os.path.join(directory, name)
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, json.JSONDecodeError):
        return {}


def rows(view: dict) -> list[dict]:
    value = view.get("rows")
    return value if isinstance(value, list) else []


def num(value: float, digits: int = 0) -> str:
    if digits:
        return f"{value:.{digits}f}"
    return f"{value:.0f}"


def signed(value: float, digits: int = 0) -> str:
    if abs(value) < 0.5 * 10 ** (-digits):
        return "0" if not digits else f"{0:.{digits}f}"
    return f"{value:+.{digits}f}" if digits else f"{value:+.0f}"


def arc(before: float, after: float, digits: int = 0) -> str:
    if before == after:
        return num(after, digits)
    if before == 0:
        return f"**+{num(after, digits)}**"
    if after == 0:
        return f"**−{num(before, digits)}**"
    return f"{num(before, digits)} → {num(after, digits)}"


def module(name: str) -> str:
    return "(root)" if name == "root" else f"`{name}/`"


def is_framework(path: str) -> bool:
    return path.endswith(FRAMEWORK_SUFFIXES)


def is_test_symbol(row: dict) -> bool:
    qname = row.get("qname", "")
    path = row.get("file", "")
    return (
        qname.startswith("tests.")
        or path.startswith(("tests/", "test/"))
        or "/tests/" in path
        or "/test/" in path
        or path.rsplit("/", 1)[-1].startswith("test_")
    )


def metric(row: dict, key: str) -> tuple[float, float]:
    old = (row.get("old") or {}).get(key, 0)
    new = (row.get("new") or {}).get(key, 0)
    return float(old or 0), float(new or 0)


def spoiler(lines: list[str]) -> list[str]:
    out = ["<details><summary>Columns and what this measures</summary>", ""]
    out.extend(f"- {line}" for line in lines)
    out.extend(["", "</details>"])
    return out


def header(meta: dict, summary: dict) -> list[str]:
    title = meta.get("title") or "code-graph report"
    sha = (meta.get("sha") or "")[:8]
    churn = f"+{meta.get('additions', 0)}/−{meta.get('deletions', 0)}"
    files = meta.get("files", 0)
    metrics = {row["metric"]: row for row in rows(summary)}

    def delta(name: str) -> float:
        return float(metrics.get(name, {}).get("delta", 0))

    node_delta = delta("nodes")
    callable_delta = delta("callables")
    edge_delta = delta("call edges")
    module_delta = delta("cross-module calls")
    if node_delta == callable_delta == edge_delta == 0:
        shape = "graph shape unchanged: no new symbols, callables or call edges"
    else:
        shape = (
            f"{signed(node_delta)} symbols, {signed(callable_delta)} callables, "
            f"{signed(edge_delta)} call edges ({signed(module_delta)} across modules)"
        )

    per_before = float(metrics.get("cyclomatic/callable", {}).get("from", 0))
    per_after = float(metrics.get("cyclomatic/callable", {}).get("to", 0))
    per_delta = per_after - per_before
    per_line = (
        f"cyclomatic/callable {per_before:.2f} → {per_after:.2f}"
        if abs(per_delta) >= 0.005
        else f"cyclomatic/callable {per_after:.2f}"
    )
    touched = metrics.get("cyclomatic/callable (changed)", {})
    touched_before = float(touched.get("from", 0))
    touched_after = float(touched.get("to", 0))
    if touched_after:
        per_line += (
            f" (touched code {touched_before:.2f} → {touched_after:.2f})"
            if abs(touched_before - touched_after) >= 0.005
            else f" (touched code {touched_after:.2f})"
        )
    added = float(metrics.get("cyclomatic/callable (added)", {}).get("to", 0))
    if added:
        per_line += f", added {added:.2f}"
    loc_delta = delta("loc (analyzed files)")

    lines = [f"## code-graph: {title}", ""]
    lines.append(f"`{sha}` · {churn} lines in {files} files · {shape}")
    lines.append("")
    affected = load(os.environ.get("VIEW_DIR", ""), "affected.json")
    reach = affected.get("totalRows", 0)
    lines.append(
        f"{reach} existing symbols reach the changed files · {per_line} · loc (analyzed) {signed(loc_delta)}"
    )
    return lines


def section_uses(view: dict, limit: int) -> list[str]:
    items = rows(view)[:limit]
    if not items:
        return []
    out = ["", "### Existing code this PR now depends on", ""]
    out.append("Existing symbols the new code starts calling.")
    out.append("")
    out.append("| existing symbol | new calls |")
    out.append("|---|---:|")
    for item in items:
        out.append(f"| `{item['symbol']}` | +{item['newCalls']} |")
    out.extend([""] + spoiler([
        "**new calls**: how many call sites the PR added into that existing symbol.",
        "A high count means the feature leans on it; for a port or shared helper that is a deepening dependency.",
        "Only new call edges count: overrides, DI wiring, tests, duplicates and re-resolutions are dropped.",
    ]))
    return out


def section_dependencies(view: dict, limit: int) -> list[str]:
    items = rows(view)[:limit]
    if not items:
        return []
    out = ["", "### What this PR depends on", ""]
    out.append("New call edges between modules, aggregated.")
    out.append("")
    out.append("| from | to | new calls |")
    out.append("|---|---|---:|")
    for item in items:
        out.append(f"| {module(item['from'])} | {module(item['to'])} | {item['newCalls']} |")
    mutual = view.get("mutual") or []
    if mutual:
        pairs = ", ".join(f"`{pair[0]} ↔ {pair[1]}`" for pair in mutual)
        out.append("")
        out.append(f"Mutual: {pairs}. Pairs through queue handlers are the job-dispatch pattern, not a cycle.")
    out.extend([""] + spoiler([
        "**from / to**: module, the first path segment of the file.",
        "**new calls**: real new call edges after dropping overrides, DI wiring, tests, duplicates and re-resolutions.",
        "**mutual**: both directions gained edges; normal for queues, worth a look for pure logic modules.",
    ]))
    return out


def section_callers(view: dict, limit: int) -> list[str]:
    items = rows(view)[:limit]
    if not items:
        return []
    out = ["", f"### Existing code that now calls into this PR ({len(rows(view))})", ""]
    out.append("Unchanged files that start calling the touched symbols.")
    out.append("")
    out.append("| existing caller | now calls |")
    out.append("|---|---|")
    for item in items:
        out.append(f"| `{item['caller']}` | `{item['callee']}` |")
    out.extend([""] + spoiler([
        "**existing caller → now calls**: a call that exists only after the PR.",
        "An unmodified file cannot add calls, so only registries and dispatch produce these rows.",
    ]))
    return out


def section_affected(view: dict, limit: int) -> list[str]:
    items = rows(view)[:limit]
    if not items:
        return []
    total = view.get("totalRows", 0)
    changed_total = view.get("affectedByChanged", 0)
    added_only = view.get("affectedByAddedOnly", 0)
    direct = view.get("direct", 0)
    out = ["", f"### Existing code affected transitively ({total})", ""]
    out.append("Unchanged callables whose execution path runs through symbols the PR touched.")
    out.append("")
    if added_only:
        out.append(f"{changed_total} depend on modified code, {added_only} reach only symbols the PR added.")
        out.append("")
    if direct and direct < total:
        out.append(f"{direct} of them call a touched symbol directly; the rest are one or more calls away.")
    elif direct:
        out.append("All of them call a touched symbol directly.")
    else:
        out.append("All of them are one or more calls away from the touched symbols.")
    out.append("")
    out.append("| existing symbol | changed | added | share | priority | direct |")
    out.append("|---|---:|---:|---:|---:|---|")
    for item in items:
        added = item.get("addedCallees") or 0
        out.append(
            f"| `{item['symbol']}` | {item['changedCallees']} | {added or ''} | "
            f"{100 * float(item['share']):.0f}% | {num(float(item['priority']))} | "
            f"{'yes' if item['direct'] else ''} |"
        )
    out.extend([""] + spoiler([
        "**changed**: touched symbols whose code the PR modified and that this callable can reach.",
        "**added**: touched symbols the PR created and that it can reach; additions are listed separately because they do not change behaviour by themselves.",
        "**share**: changed / all reachable callees — how much of the function's execution surface the PR rewrote.",
        "**priority**: refactor priority of the unchanged function; high share with high priority means read it first.",
        "**direct**: it calls a touched symbol itself, not through an intermediate.",
        "Call edges only: shared config, DTO and constant data flow is not counted, so this is a lower bound.",
    ]))
    return out


def why(row: dict) -> str:
    new = row.get("new") or {}
    old = row.get("old") or {}
    delta = row.get("delta") or {}
    status = row.get("status")
    reasons = []
    if new.get("deadCodeCandidate"):
        reasons.append("route/DI call not in graph" if is_framework(row.get("file", "")) else "no direct caller")
    if status == "added":
        cyclomatic = int(new.get("cyclomatic", 0) or 0)
    else:
        cyclomatic = int(delta.get("cyclomatic", 0) or 0)
    if status == "removed" and int(old.get("cyclomatic", 0) or 0) >= 2:
        reasons.append(f"was {old.get('cyclomatic', 0)} branches")
    elif cyclomatic >= 2:
        reasons.append(f"complexity {signed(cyclomatic)}")
    if status == "added" and new.get("loc", 0) >= 40 and new.get("cyclomatic", 0) < 5:
        reasons.append(f"{new.get('loc', 0)} lines")
    priority = float(delta.get("priority", 0) or 0)
    if status in ("changed", "moved") and abs(priority) >= 5:
        reasons.append(f"priority {signed(priority, 1)}")
    if reasons:
        return ", ".join(reasons[:3])
    return {"added": "new", "removed": "removed"}.get(status, "")


def review_row(row: dict) -> str:
    before_cx, after_cx = metric(row, "cyclomatic")
    before_loc, after_loc = metric(row, "loc")
    before_fan, after_fan = metric(row, "callFanIn")
    before_priority, after_priority = metric(row, "priority")
    return (
        f"| `{row['qname']}` | {row['status']} | {arc(before_cx, after_cx)} | "
        f"{arc(before_loc, after_loc)} | {arc(before_fan, after_fan)} | "
        f"{arc(before_priority, after_priority, 1)} | {why(row)} |"
    )


def section_review(view: dict, limit: int) -> list[str]:
    items = [
        row
        for row in rows(view)
        if row.get("kind") not in ("constant", "constructor")
        and not row.get("file", "").endswith(".spec.ts")
        and not is_test_symbol(row)
    ][:limit]
    if not items:
        return []
    total = view.get("totalRows", len(rows(view)))
    out = ["", f"### Review these ({len(items)} of {total})", ""]
    out.append("Symbols worth reading first.")
    out.append("")
    out.append("| symbol | status | complexity | loc | callers | priority | why |")
    out.append("|---|---|---|---:|---:|---:|---|")
    out.extend(review_row(row) for row in items)
    out.extend([""] + spoiler([
        "**status**: added, changed or removed; added and removed rows show signed values instead of a delta.",
        "**complexity / loc / callers**: cyclomatic, lines and incoming calls.",
        "**priority**: refactor priority, `A → B` for changed symbols and a signed value for new or removed ones.",
        "**why**: why the row is listed — size, complexity jump, removal, missing caller.",
        "Constants, synthetic module constructors and test symbols are not listed.",
    ]))
    return out


def section_areas(view: dict, limit: int) -> list[str]:
    items = rows(view)[:limit]
    if len(items) <= 1:
        return []
    out = ["", "### By area", ""]
    out.append("Where the change lands.")
    out.append("")
    out.append("| area | + / − / ~ | cyclomatic/callable | Δloc |")
    out.append("|---|---|---:|---:|")
    for item in items:
        before = float(item.get("cyclomaticBefore", 0))
        after = float(item.get("cyclomaticAfter", 0))
        cx = f"{after:.2f}" if abs(before - after) < 0.005 else f"{before:.2f} → {after:.2f}"
        out.append(
            f"| {module(item['area'])} | +{item['added']} / −{item['removed']} / ~{item['changed']} | "
            f"{cx} | {signed(float(item['locDelta']))} |"
        )
    out.extend([""] + spoiler([
        "**+ / − / ~**: symbols added, removed and changed in that area.",
        "**cyclomatic/callable**: mean cyclomatic; a `→` appears only when it moved. Absolute sums grow with file count.",
        "**Δloc**: absolute line delta, which tracks effort rather than complexity.",
    ]))
    return out


def section_details(view: dict, limit: int) -> list[str]:
    items = rows(view)[:limit]
    if not items:
        return []
    total = view.get("totalRows", len(items))
    out = ["", f"<details><summary>{total} symbol changes</summary>", ""]
    out.append("")
    out.append("| symbol | status | kind | Δcomplexity | Δloc | Δpriority |")
    out.append("|---|---|---|---:|---:|---:|")
    for row in items:
        before_cx, after_cx = metric(row, "cyclomatic")
        before_loc, after_loc = metric(row, "loc")
        before_priority, after_priority = metric(row, "priority")
        out.append(
            f"| `{row['qname']}` | {row['status']} | {row['kind']} | "
            f"{arc(before_cx, after_cx)} | {arc(before_loc, after_loc)} | "
            f"{arc(before_priority, after_priority, 1)} |"
        )
    out.append("")
    out.append("</details>")
    return out


def section_footnotes(filtered: dict, silent: dict, dead: dict, warnings: str) -> list[str]:
    out: list[str] = []
    funnel = rows(filtered)
    used = next((row["edges"] for row in funnel if row["reason"] == "used"), 0)
    total = filtered.get("newCallEdges", used)
    if total:
        dropped = [f"{row['edges']} {row['reason']}" for row in funnel if row["reason"] != "used" and row["edges"]]
        line = f"{total} new call edges, {used} used"
        if dropped:
            line += "; dropped " + ", ".join(dropped)
        out.append(line + ".")
    silent_total = silent.get("totalRows", len(rows(silent)))
    if silent_total:
        out.append(f"{silent_total} changed files carry no symbol rows (docs, config, scripts, non-code).")
    dead_total = dead.get("totalRows", len(rows(dead)))
    if dead_total:
        framework = sum(1 for row in rows(dead) if row.get("framework"))
        out.append(
            f"{dead_total} dead-code candidates appeared"
            + (f", {framework} in DI/route files where decorators are invisible to the graph" if framework else "")
            + "."
        )
    if warnings.strip():
        out.extend(["", "<details><summary>diff warnings</summary>", "", "```text", warnings.strip(), "```", "", "</details>"])
    if out:
        out.insert(0, "")
    return out


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", required=True)
    parser.add_argument("--title", default="")
    parser.add_argument("--sha", default="")
    parser.add_argument("--additions", type=int, default=0)
    parser.add_argument("--deletions", type=int, default=0)
    parser.add_argument("--files", type=int, default=0)
    parser.add_argument("--limit", type=int, default=8)
    args = parser.parse_args()

    os.environ["VIEW_DIR"] = args.dir
    views = {name: load(args.dir, f"{name}.json") for name in (
        "summary", "uses", "dependencies", "callers", "affected",
        "review", "areas", "symbols", "filtered", "silent", "dead",
    )}
    summary = views["summary"]
    if not rows(summary):
        print("_Analysis did not run — see the job log._")
        return 0

    # Files with no symbols and diff warnings alone are not a review: the graph did
    # not move. Exit code 3 tells the workflow to post nothing.
    if not (
        rows(views["uses"])
        or rows(views["dependencies"])
        or rows(views["callers"])
        or rows(views["affected"])
        or rows(views["symbols"])
        or rows(views["dead"])
        or len(rows(views["areas"])) > 1
        or views["filtered"].get("newCallEdges", 0)
    ):
        return 3

    meta = {
        "title": args.title,
        "sha": args.sha,
        "additions": args.additions,
        "deletions": args.deletions,
        "files": args.files,
    }
    lines: list[str] = header(meta, summary)
    lines += section_uses(views["uses"], 10)
    lines += section_dependencies(views["dependencies"], 6)
    lines += section_callers(views["callers"], 8)
    lines += section_affected(views["affected"], args.limit)
    lines += section_review(views["review"], 12)
    lines += section_areas(views["areas"], 6)
    lines += section_details(views["symbols"], 80)
    warnings = ""
    try:
        warnings = open(os.path.join(args.dir, "warnings.log"), encoding="utf-8").read()
    except OSError:
        pass
    lines += section_footnotes(views["filtered"], views["silent"], views["dead"], warnings)
    print("\n".join(lines).strip() + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
