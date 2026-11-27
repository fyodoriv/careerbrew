//! `charts.Report`: self-contained HTML with inline SVG charts (no network,
//! no script). The markup and CSS are byte-for-byte the Python template's.

pub const CSS: &str = r#"
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
"#;

/// `html.escape(str(x))`.
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            other => out.push(other),
        }
    }
    out
}

/// The value formats the Python callers pass: `{:.0%}` and `{:.0f}`.
#[derive(Clone, Copy)]
pub enum Fmt {
    Percent0,
    Fixed0,
}

impl Fmt {
    pub fn parse(text: &str) -> Option<Fmt> {
        match text {
            "{:.0%}" => Some(Fmt::Percent0),
            "{:.0f}" => Some(Fmt::Fixed0),
            _ => None,
        }
    }

    pub fn format(self, value: f64) -> String {
        match self {
            Fmt::Percent0 => format!("{:.0}%", value * 100.0),
            Fmt::Fixed0 => format!("{value:.0}"),
        }
    }
}

fn clamp01(x: f64) -> f64 {
    // Python `max(0.0, min(1.0, x))`.
    let low = if x < 1.0 { x } else { 1.0 };
    if low > 0.0 {
        low
    } else {
        0.0
    }
}

pub struct Report {
    pub title: String,
    pub subtitle: String,
    parts: Vec<String>,
}

impl Report {
    pub fn new(title: &str, subtitle: &str) -> Report {
        Report {
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            parts: Vec::new(),
        }
    }

    pub fn kpis(&mut self, items: &[(String, String)]) {
        let body = items
            .iter()
            .map(|(k, v)| format!(r#"<span class="kpi"><b>{}</b>{}</span>"#, esc(v), esc(k)))
            .collect::<String>();
        self.parts.push(format!("<div>{body}</div>"));
    }

    pub fn text(&mut self, lines: &[String]) {
        let body = lines
            .iter()
            .map(|line| format!("<li>{}</li>", esc(line)))
            .collect::<String>();
        self.parts.push(format!("<ul>{body}</ul>"));
    }

    pub fn heading(&mut self, text: &str) {
        self.parts.push(format!("<h2>{}</h2>", esc(text)));
    }

    pub fn bars(&mut self, title: &str, rows: &[(String, f64)], max_value: f64, fmt: Fmt) {
        self.heading(title);
        if rows.is_empty() {
            self.parts.push(r#"<p class="sub">No data yet.</p>"#.into());
            return;
        }
        let (row_h, label_w, width) = (26_i64, 170_i64, 800_i64);
        let h = row_h * rows.len() as i64 + 24;
        let plot_w = (width - label_w - 60) as f64;
        let mut out = vec![format!(
            r#"<svg viewBox="0 0 {width} {h}" role="img" aria-label="{}">"#,
            esc(title)
        )];
        for frac in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let x = label_w as f64 + plot_w * frac;
            out.push(format!(
                r#"<line class="grid" x1="{x:.1}" y1="0" x2="{x:.1}" y2="{}"/>"#,
                h - 18
            ));
            out.push(format!(
                r#"<text x="{x:.1}" y="{}" text-anchor="middle">{}</text>"#,
                h - 4,
                esc(&fmt.format(max_value * frac))
            ));
        }
        for (i, (label, value)) in rows.iter().enumerate() {
            let y = i as i64 * row_h + 4;
            let ratio = if max_value != 0.0 {
                value / max_value
            } else {
                0.0
            };
            let w = clamp01(ratio) * plot_w;
            let shown = esc(&fmt.format(*value));
            out.push(format!(
                r#"<text x="{}" y="{}" text-anchor="end">{}</text>"#,
                label_w - 8,
                y + 15,
                esc(label)
            ));
            out.push(format!(
                r#"<rect class="mark" x="{label_w}" y="{}" width="{:.1}" height="16" rx="4"><title>{}: {shown}</title></rect>"#,
                y + 3,
                if w > 0.5 { w } else { 0.5 },
                esc(label)
            ));
            out.push(format!(
                r#"<text x="{:.1}" y="{}">{shown}</text>"#,
                label_w as f64 + w + 6.0,
                y + 15
            ));
        }
        out.push("</svg>".into());
        self.parts.push(out.concat());
        let table = rows
            .iter()
            .map(|(label, value)| (label.clone(), fmt.format(*value)))
            .collect::<Vec<_>>();
        self.table(&["item", "value"], &table);
    }

    pub fn line(&mut self, title: &str, points: &[(String, f64)], y_max: f64, fmt: Fmt) {
        self.heading(title);
        if points.is_empty() {
            self.parts.push(r#"<p class="sub">No data yet.</p>"#.into());
            return;
        }
        let (width, h, left, bottom) = (800_i64, 220_i64, 48_i64, 24_i64);
        let (plot_w, plot_h) = (width - left - 16, h - bottom - 10);
        let mut out = vec![format!(
            r#"<svg viewBox="0 0 {width} {h}" role="img" aria-label="{}">"#,
            esc(title)
        )];
        for frac in [0.0, 0.5, 1.0] {
            let y = 10.0 + plot_h as f64 * (1.0 - frac);
            out.push(format!(
                r#"<line class="grid" x1="{left}" y1="{y:.1}" x2="{}" y2="{y:.1}"/>"#,
                width - 16
            ));
            out.push(format!(
                r#"<text x="{}" y="{:.1}" text-anchor="end">{}</text>"#,
                left - 6,
                y + 4.0,
                esc(&fmt.format(y_max * frac))
            ));
        }
        let n = points.len();
        let xy = points
            .iter()
            .enumerate()
            .map(|(i, (_, v))| {
                let offset = if n > 1 {
                    (plot_w * i as i64) as f64 / (n - 1) as f64
                } else {
                    plot_w as f64 / 2.0
                };
                let x = left as f64 + offset;
                let ratio = if y_max != 0.0 { v / y_max } else { 0.0 };
                let y = 10.0 + plot_h as f64 * (1.0 - clamp01(ratio));
                (x, y)
            })
            .collect::<Vec<_>>();
        let joined = xy
            .iter()
            .map(|(x, y)| format!("{x:.1},{y:.1}"))
            .collect::<Vec<_>>()
            .join(" ");
        out.push(format!(r#"<polyline class="line" points="{joined}"/>"#));
        for ((x, y), (label, v)) in xy.iter().zip(points) {
            out.push(format!(
                r#"<circle class="dot" cx="{x:.1}" cy="{y:.1}" r="5"><title>{}: {}</title></circle>"#,
                esc(label),
                esc(&fmt.format(*v))
            ));
        }
        out.push(format!(
            r#"<text x="{left}" y="{}">{}</text>"#,
            h - 4,
            esc(&points[0].0)
        ));
        out.push(format!(
            r#"<text x="{}" y="{}" text-anchor="end">{}</text>"#,
            width - 16,
            h - 4,
            esc(&points[n - 1].0)
        ));
        out.push("</svg>".into());
        self.parts.push(out.concat());
        let table = points
            .iter()
            .map(|(label, v)| (label.clone(), fmt.format(*v)))
            .collect::<Vec<_>>();
        self.table(&["when", "value"], &table);
    }

    fn table(&mut self, head: &[&str], rows: &[(String, String)]) {
        let body = rows
            .iter()
            .map(|(a, b)| format!("<tr><td>{}</td><td>{}</td></tr>", esc(a), esc(b)))
            .collect::<String>();
        let head = head
            .iter()
            .map(|h| format!("<th>{}</th>", esc(h)))
            .collect::<String>();
        self.parts.push(format!(
            "<details><summary>Table view</summary><table><tr>{head}</tr>{body}</table></details>"
        ));
    }

    pub fn html(&self) -> String {
        format!(
            concat!(
                r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">"#,
                r#"<title>{}</title><style>{}</style></head><body><main class="viz-root">"#,
                r#"<h1>{}</h1><p class="sub">{}</p>{}</main></body></html>"#
            ),
            esc(&self.title),
            CSS,
            esc(&self.title),
            esc(&self.subtitle),
            self.parts.concat()
        )
    }
}
