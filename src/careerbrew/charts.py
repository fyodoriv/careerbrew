"""Self-contained HTML reports with inline SVG charts (no network, no JS libraries).

Follows the dataviz method: one axis per chart, thin marks, 4px rounded bar
ends, 2px lines, >= 8px markers, recessive grid, text in text tokens, a
hover tooltip per mark (SVG <title>), a table view under every chart, and
selected light/dark tokens (reference palette slot 1).
"""

from __future__ import annotations

import html
from collections.abc import Sequence
from dataclasses import dataclass, field

CSS = """
.viz-root{color-scheme:light;--surface-1:#fcfcfb;--text-primary:#0b0b0b;--text-secondary:#52514e;
--grid:#e4e3df;--series-1:#2a78d6;--good:#008300;--critical:#e34948;
font:14px/1.45 -apple-system,system-ui,sans-serif;background:var(--surface-1);color:var(--text-primary);
max-width:860px;margin:0 auto;padding:16px}
@media (prefers-color-scheme:dark){:root:where(:not([data-theme="light"])) .viz-root{color-scheme:dark;
--surface-1:#1a1a19;--text-primary:#f0efec;--text-secondary:#c3c2b7;--grid:#3a3936;--series-1:#3987e5;
--good:#3fa33f;--critical:#e66767}}
:root[data-theme="dark"] .viz-root{color-scheme:dark;--surface-1:#1a1a19;--text-primary:#f0efec;
--text-secondary:#c3c2b7;--grid:#3a3936;--series-1:#3987e5;--good:#3fa33f;--critical:#e66767}
body{margin:0;background:#fcfcfb}@media (prefers-color-scheme:dark){body{background:#1a1a19}}
h1{font-size:20px}h2{font-size:16px;margin-top:28px}.sub{color:var(--text-secondary)}
svg{width:100%;height:auto;display:block}svg text{fill:var(--text-secondary);font-size:11px}
.mark{fill:var(--series-1)}.line{stroke:var(--series-1);stroke-width:2;fill:none}
.grid{stroke:var(--grid);stroke-width:1}.dot{fill:var(--series-1);stroke:var(--surface-1);stroke-width:2}
.mark:hover,.dot:hover{opacity:.8}table{border-collapse:collapse;margin-top:6px;font-size:12px}
td,th{padding:2px 10px 2px 0;text-align:left;border-bottom:1px solid var(--grid)}
details{margin-top:4px}.kpi{display:inline-block;margin-right:24px}.kpi b{font-size:22px;display:block}
.status-good{color:var(--good)}.status-critical{color:var(--critical)}
"""


def esc(x: object) -> str:
    return html.escape(str(x))


@dataclass
class Report:
    title: str
    subtitle: str = ""
    parts: list[str] = field(default_factory=list)

    def kpis(self, items: list[tuple[str, str]]) -> None:
        self.parts.append("<div>" + "".join(f'<span class="kpi"><b>{esc(v)}</b>{esc(k)}</span>' for k, v in items) + "</div>")

    def text(self, md_lines: list[str]) -> None:
        self.parts.append("<ul>" + "".join(f"<li>{esc(line)}</li>" for line in md_lines) + "</ul>")

    def heading(self, text: str) -> None:
        self.parts.append(f"<h2>{esc(text)}</h2>")

    def bars(self, title: str, rows: Sequence[tuple[str, float]], max_value: float = 1.0, fmt: str = "{:.0%}") -> None:
        """Horizontal bars (magnitude by category), baseline at zero."""
        self.heading(title)
        if not rows:
            self.parts.append('<p class="sub">No data yet.</p>')
            return
        row_h, label_w, width = 26, 170, 800
        h = row_h * len(rows) + 24
        plot_w = width - label_w - 60
        out = [f'<svg viewBox="0 0 {width} {h}" role="img" aria-label="{esc(title)}">']
        for frac in (0, 0.25, 0.5, 0.75, 1.0):
            x = label_w + plot_w * frac
            out.append(f'<line class="grid" x1="{x:.1f}" y1="0" x2="{x:.1f}" y2="{h - 18}"/>')
            out.append(f'<text x="{x:.1f}" y="{h - 4}" text-anchor="middle">{esc(fmt.format(max_value * frac))}</text>')
        for i, (label, value) in enumerate(rows):
            y = i * row_h + 4
            w = max(0.0, min(1.0, value / max_value if max_value else 0)) * plot_w
            out.append(f'<text x="{label_w - 8}" y="{y + 15}" text-anchor="end">{esc(label)}</text>')
            out.append(f'<rect class="mark" x="{label_w}" y="{y + 3}" width="{max(w, 0.5):.1f}" height="16" rx="4"><title>{esc(label)}: {esc(fmt.format(value))}</title></rect>')
            out.append(f'<text x="{label_w + w + 6:.1f}" y="{y + 15}">{esc(fmt.format(value))}</text>')
        out.append("</svg>")
        self.parts.append("".join(out))
        self._table(["item", "value"], [(r[0], fmt.format(r[1])) for r in rows])

    def line(self, title: str, points: list[tuple[str, float]], y_max: float = 1.0, fmt: str = "{:.0%}") -> None:
        """One series over time (x labels are ordered strings such as dates)."""
        self.heading(title)
        if not points:
            self.parts.append('<p class="sub">No data yet.</p>')
            return
        width, h, left, bottom = 800, 220, 48, 24
        plot_w, plot_h = width - left - 16, h - bottom - 10
        out = [f'<svg viewBox="0 0 {width} {h}" role="img" aria-label="{esc(title)}">']
        for frac in (0, 0.5, 1.0):
            y = 10 + plot_h * (1 - frac)
            out.append(f'<line class="grid" x1="{left}" y1="{y:.1f}" x2="{width - 16}" y2="{y:.1f}"/>')
            out.append(f'<text x="{left - 6}" y="{y + 4:.1f}" text-anchor="end">{esc(fmt.format(y_max * frac))}</text>')
        n = len(points)
        xy = []
        for i, (_label, v) in enumerate(points):
            x = left + (plot_w * i / (n - 1) if n > 1 else plot_w / 2)
            y = 10 + plot_h * (1 - max(0.0, min(1.0, v / y_max if y_max else 0)))
            xy.append((x, y))
        out.append('<polyline class="line" points="' + " ".join(f"{x:.1f},{y:.1f}" for x, y in xy) + '"/>')
        for (x, y), (label, v) in zip(xy, points, strict=True):
            out.append(f'<circle class="dot" cx="{x:.1f}" cy="{y:.1f}" r="5"><title>{esc(label)}: {esc(fmt.format(v))}</title></circle>')
        out.append(f'<text x="{left}" y="{h - 4}">{esc(points[0][0])}</text>')
        out.append(f'<text x="{width - 16}" y="{h - 4}" text-anchor="end">{esc(points[-1][0])}</text>')
        out.append("</svg>")
        self.parts.append("".join(out))
        self._table(["when", "value"], [(p[0], fmt.format(p[1])) for p in points])

    def _table(self, head: list[str], rows: list[tuple[str, str]]) -> None:
        body = "".join("<tr>" + "".join(f"<td>{esc(c)}</td>" for c in r) + "</tr>" for r in rows)
        self.parts.append(f"<details><summary>Table view</summary><table><tr>{''.join(f'<th>{esc(h)}</th>' for h in head)}</tr>{body}</table></details>")

    def html(self) -> str:
        return (
            f'<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">'
            f'<title>{esc(self.title)}</title><style>{CSS}</style></head><body><main class="viz-root">'
            f'<h1>{esc(self.title)}</h1><p class="sub">{esc(self.subtitle)}</p>{"".join(self.parts)}</main></body></html>'
        )
