#!/usr/bin/env python3
"""Render the small markdown subset our report variants emit into HTML."""
from __future__ import annotations

import html
import re


def _inline(text: str) -> str:
    out = html.escape(text, quote=False)
    out = re.sub(r"`([^`]+)`", r"<code>\1</code>", out)
    out = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", out)
    out = re.sub(r"(?<![\w*])\*([^*]+)\*(?![\w*])", r"<em>\1</em>", out)
    out = re.sub(r"\[([^\]]+)\]\((https?://[^)]+)\)", r'<a href="\2">\1</a>', out)
    return out


def render(markdown: str) -> str:
    lines = markdown.split("\n")
    out: list[str] = []
    i = 0
    in_table = False
    in_list = False
    in_details = False
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        if stripped.startswith("<details>"):
            out.append("<details>")
            in_details = True
            i += 1
            continue
        if stripped.startswith("<summary>"):
            out.append(f"<summary>{_inline(stripped[len('<summary>'):-len('</summary>')])}</summary>")
            i += 1
            continue
        if stripped == "</details>":
            out.append("</details>")
            in_details = False
            i += 1
            continue
        if stripped.startswith("```"):
            lang = stripped[3:].strip()
            i += 1
            body = []
            while i < len(lines) and not lines[i].strip().startswith("```"):
                body.append(html.escape(lines[i]))
                i += 1
            i += 1
            out.append(f'<pre class="code"><code>{chr(10).join(body)}</code></pre>')
            continue

        if stripped.startswith("#"):
            level = len(stripped) - len(stripped.lstrip("#"))
            level = min(level, 6)
            out.append(f"<h{level}>{_inline(stripped[level:].strip())}</h{level}>")
            i += 1
            continue

        if stripped.startswith("|") and stripped.endswith("|"):
            if not in_table:
                out.append('<table class="md-table">')
                in_table = True
                header = [c.strip() for c in stripped.strip("|").split("|")]
                out.append("<thead><tr>" + "".join(f"<th>{_inline(c)}</th>" for c in header) + "</tr></thead><tbody>")
                i += 2  # skip the separator row
                continue
            cells = [c.strip() for c in stripped.strip("|").split("|")]
            out.append("<tr>" + "".join(f"<td>{_inline(c)}</td>" for c in cells) + "</tr>")
            i += 1
            continue
        if in_table:
            out.append("</tbody></table>")
            in_table = False

        if stripped in ("---", "***"):
            out.append("<hr>")
            i += 1
            continue

        if stripped.startswith(("- ", "* ")):
            if not in_list:
                out.append("<ul>")
                in_list = True
            out.append(f"<li>{_inline(stripped[2:])}</li>")
            i += 1
            continue
        if re.match(r"^\d+\. ", stripped):
            if not in_list:
                out.append("<ol>")
                in_list = True
            out.append(f"<li>{_inline(re.sub(r'^\d+\. ', '', stripped))}</li>")
            i += 1
            continue
        if in_list:
            out.append("</ul>")
            in_list = False

        if not stripped:
            i += 1
            continue

        out.append(f"<p>{_inline(stripped)}</p>")
        i += 1

    if in_table:
        out.append("</tbody></table>")
    if in_list:
        out.append("</ul>")
    return "\n".join(out)
