#!/usr/bin/env python3
"""Print the source behind each claim a report would make, for eyeball validation.

Usage: python3 verify.py <pr-number> [max-claims]
"""
from __future__ import annotations

import os
import re
import sys

import lib

FRAMEWORK = re.compile(r"\.(controller|module|provider|guard|interceptor)\.ts$")
ROOT = "/tmp/cgpr"


def snippet(path: str, line: int, end_line: int, pad: int = 3) -> str:
    try:
        lines = open(path, encoding="utf-8").read().split("\n")
    except OSError:
        return "<missing>"
    start = max(1, (line or 1) - pad)
    end = min(len(lines), (end_line or line or 1) + pad)
    out = []
    for number in range(start, end + 1):
        out.append(f"{number:5d}  {lines[number - 1]}")
    return "\n".join(out)


def main() -> None:
    pr = int(sys.argv[1])
    limit = int(sys.argv[2]) if len(sys.argv) > 2 else 8
    pd = lib.load(pr)
    root = f"{ROOT}/{pr}/head/backend/src"
    claims = []

    for row in pd.rows:
        if row.status == "added" and row.after.get("deadCodeCandidate") and not FRAMEWORK.search(row.file):
            claims.append(("dead-code candidate (non-framework)", row, row.after))
        if row.d("cyclomatic") >= 3:
            claims.append((f"complexity +{row.d('cyclomatic'):.0f}", row, row.after))
        if row.status == "added" and row.after.get("cyclomatic", 0) >= 8:
            claims.append((f"new symbol with complexity {row.after.get('cyclomatic', 0):.0f}", row, row.after))

    print(f"PR #{pd.number} {pd.title}: {len(claims)} claims to verify")
    for reason, row, metrics in claims[:limit]:
        node = pd.head_nodes.get(row.qname) or {}
        line = row.line or 1
        end = None
        for node_data in pd.head_nodes.values():
            if node_data["qname"] == row.qname and node_data["file"] == row.file:
                line, end = node_data.get("line"), node_data.get("endLine")
                break
        print(f"\n--- {reason}: {row.qname}  ({row.file}:{line})")
        print(snippet(os.path.join(root, row.file), line, end))


if __name__ == "__main__":
    main()
