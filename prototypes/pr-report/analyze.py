#!/usr/bin/env python3
"""Cross-PR noise analysis: what the shipped report would show and where it misleads."""
from __future__ import annotations

import os
import sys

import generate as g
import lib


def describe(n: int) -> dict:
    pd = lib.load(n)
    ctx = g.Ctx(pd)
    zero = [r for r in pd.rows if r.status == "changed"
            and all(abs(r.d(k)) < 1e-9 for k in ("refactorPriority", "cyclomatic", "loc", "callFanIn", "ifc"))]
    spec = [r for r in pd.rows if r.file.endswith(".spec.ts")]
    consts = [r for r in pd.rows if r.kind == "constant"]
    synth = [r for r in pd.rows if r.synthetic]
    fw_dead = [r for r in ctx.new_dead if g.FRAMEWORK_FILE.search(r.file)]
    real_dead = [r for r in ctx.new_dead if not g.FRAMEWORK_FILE.search(r.file)]
    new_cycles = [c for c in ctx.cycles_after if c not in ctx.cycles_before]
    return {
        "n": n,
        "title": pd.title,
        "files": len(pd.changed),
        "plus": pd.additions,
        "minus": pd.deletions,
        "rows": len(pd.rows),
        "added": ctx.by_status.get("added", 0),
        "removed": ctx.by_status.get("removed", 0),
        "changed": ctx.by_status.get("changed", 0),
        "zero_delta": len(zero),
        "spec_rows": len(spec),
        "const_rows": len(consts),
        "synthetic_rows": len(synth),
        "noise_rows": len(spec) + len(consts) + len(synth),
        "silent_files": len(ctx.silent_files),
        "new_dead": len(ctx.new_dead),
        "fw_dead": len(fw_dead),
        "real_dead": len(real_dead),
        "new_cycles": len(new_cycles),
        "d_cx": ctx.ta["complexity"] - ctx.tb["complexity"],
        "d_loc": ctx.ta["loc"] - ctx.tb["loc"],
        "d_priority": ctx.ta["priority"] - ctx.tb["priority"],
    }


def main() -> None:
    numbers = [int(a) for a in sys.argv[1:]] or sorted(
        int(name) for name in os.listdir(lib.DATA)
        if name.isdigit() and os.path.exists(f"{lib.DATA}/{name}/diff.json"))
    header = ("pr", "files", "+/−", "rows", "add", "del", "chg", "const", "synth", "spec",
              "noise%", "silent", "newDead", "fw", "real", "newCyc", "Δcx", "Δloc")
    print("\t".join(header))
    for n in numbers:
        d = describe(n)
        print("\t".join(str(v) for v in (
            f"#{d['n']}", d["files"], f"+{d['plus']}/-{d['minus']}", d["rows"], d["added"],
            d["removed"], d["changed"], d["const_rows"], d["synthetic_rows"], d["spec_rows"],
            f"{100 * d['noise_rows'] / max(1, d['rows']):.0f}%", d["silent_files"],
            d["new_dead"], d["fw_dead"], d["real_dead"], d["new_cycles"], d["d_cx"], d["d_loc"])))
        print(f"    {d['title']}")


if __name__ == "__main__":
    main()
