import { About } from "./about";
import { api, localStamp } from "./api";
import { PAGE_SIZE } from "./order";
import { Paged } from "./pager";
import { useData } from "./useData";
import { keepDates } from "./text";
import { Empty, Section, State } from "./views";

// Run pages (scripts/checkup.sh, `careerbrew`, review, work collect, day plan, checkup, machine ready).
// Each page is self-contained HTML with no script; it shows in a sandboxed frame (no scripts, no same origin),
// so its styles never touch the app and it can never reach the token.
export function ReportsView({ id }: { id?: string }) {
  const q = useData(api.reports);
  const current = id ?? q.data?.reports[0]?.id;
  const page = useData(() => (current ? api.report(current) : Promise.resolve(null)), [current], { key: current ? `/api/reports/${encodeURIComponent(current)}` : null });
  return (
    <div className="split split-reader">
      <State q={q} label="run pages">
        {(d) => (
          <Section title={`Run pages (${d.reports.length})`} className="pane" block="runs">
            {d.reports.length === 0 ? (
              <Empty>No run pages yet. Each long run ends with one: <code>scripts/checkup.sh</code>, <code>careerbrew</code>, <code>careerbrew review</code>, <code>work collect</code>, <code>day plan</code>.</Empty>
            ) : (
              // Newest first, 10 at a time; with no page in the hash, the page that holds the open run.
              <Paged id="runs" rows={d.reports} size={PAGE_SIZE.runs} noun="run pages" focus={(r) => r.id === current}>
              {(rows) => <ul className="rows report-list">
                {rows.map((r) => (
                  <li key={r.id}>
                    <a href={`#/reports/${encodeURIComponent(r.id)}`} aria-current={r.id === current ? "page" : undefined}><span>{keepDates(r.title)}</span></a>
                    <div className="muted small">
                      <span className={`tag ${r.status === "blocked" ? "tag-no" : "tag-yes"}`}>{r.status}</span>
                      {keepDates(localStamp(r.at))}, {r.done.length} done, {r.blockers.length} blocker{r.blockers.length === 1 ? "" : "s"}, {r.next.length} next
                    </div>
                  </li>
                ))}
              </ul>}
              </Paged>
            )}
          </Section>
        )}
      </State>
      {current ? (
        <State q={page} label="the run page">
          {(p) => p && (
            <article className="card report-reader" aria-label={p.summary.title} data-block="reader">
              <About block="reader" />
              <iframe className="report-frame" title={`Run page: ${p.summary.title}`} sandbox="" srcDoc={p.html} />
            </article>
          )}
        </State>
      ) : (
        <div className="card reader reader-empty" data-block="reader"><About block="reader" /><Empty>Pick a run page to read it here.</Empty></div>
      )}
    </div>
  );
}
