import { useEffect, useLayoutEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { CSSProperties, ReactNode } from "react";
import { api, duration, hhmm, localDate, pct } from "./api";
import type { FeedAsk, FocusBlock, PriorityRow, Horizon, Jobs, Meeting, Metric, Prep, Rating, ScheduleItem, Status, TierRow, WatchedProject } from "./api";
import { About } from "./about";
import { AgentActivity } from "./agents";
import { explain, useAct } from "./act";
import { ActStatus } from "./act-status";
import { DayLog, EarlierDays, InboxBlock, PmCopyBar, PmUpdate, SettingsEditor } from "./daylog";
import { TaskLinks, TaskWhy } from "./links";
import { PAGE_SIZE } from "./order";
import { Paged } from "./pager";
import { useHashState } from "./router";
import { keepDates, withCode } from "./text";
import { useData } from "./useData";
import { FeedHealthNotice, SourceAge } from "./feedHealth";
import { useFeedHealth } from "./useFeedHealth";
import type { Loadable } from "./useData";
import { useDayLog } from "./useDayLog";
import { friendlyDay, shortDay } from "./dates";
import { VoicePractice } from "./voice";

// ---------- shared pieces ----------

// `block` names a view's top-level block (`data-block`, issue #146): VIEW_BLOCKS in order.ts lists them in order.
// A block says what it is under its title ("What is this?", issue #147).
export function Section({ title, children, action, className, block }: { title: string; children: ReactNode; action?: ReactNode; className?: string; block?: string }) {
  return (
    <section className={`card${className ? ` ${className}` : ""}`} aria-label={title} data-block={block}>
      <header className="card-head">
        <h2>{title}</h2>
        {action}
      </header>
      {block && <About block={block} />}
      {children}
    </section>
  );
}

// A table that never scrolls sideways (issue #67). When its box is narrower than 560 px, styles.css stacks it
// into one block per row. Each cell then shows its column name from `data-label`, which this copies from the
// header after every render.
export function DataTable({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLTableElement>(null);
  useLayoutEffect(() => {
    const table = ref.current;
    if (!table) return;
    const names = Array.from(table.querySelectorAll("thead th"), (th) => th.textContent ?? "");
    for (const row of table.querySelectorAll("tbody tr")) Array.from(row.children).forEach((cell, i) => cell.setAttribute("data-label", names[i] ?? ""));
  });
  return <div className="table-wrap"><table ref={ref}>{children}</table></div>;
}

// Every block of a page loads, fails, and refreshes on its own (issue #142). While it loads: a visible
// "Loading <label>…", with the seconds once it takes 3 s or more. When it fails: an error with its fix and a
// Retry button, and the rest of the page stays. With the last answer cached: that answer at once, marked
// "Updated hh:mm", while the fresh one loads.
export function State<T>({ q, label, compact, children }: { q: Loadable<T>; label: string; compact?: boolean; children: (d: T) => ReactNode }) {
  if (q.data === null) {
    if (q.error && !q.loading) return <BlockError label={label} error={q.error} retry={q.reload} />;
    return <Loading label={label} compact={compact} />;
  }
  return (
    <>
      {q.error && !q.loading && <BlockError label={label} error={q.error} retry={q.reload} />}
      <CachedNote q={q} />
      {children(q.data)}
    </>
  );
}

// "Updated hh:mm" while a block shows its cached answer: refreshing, or not refreshed when that failed.
export function CachedNote({ q }: { q: Pick<Loadable<unknown>, "cached" | "loading"> }) {
  if (!q.cached) return null;
  return <p className="muted small updated">Updated {clock(q.cached.at)} · {q.loading ? (q.cached.stale ? "new data, refreshing…" : "refreshing…") : "not refreshed"}</p>;
}

function clock(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function Loading({ label, compact }: { label: string; compact?: boolean }) {
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    const start = Date.now();
    const timer = window.setInterval(() => setSeconds(Math.floor((Date.now() - start) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, []);
  return (
    <div className={`skeleton${compact ? " skeleton-compact" : ""}`} aria-busy="true">
      <span className="skeleton-label" aria-live="polite">Loading {label}…{seconds >= 3 ? ` ${seconds} s` : ""}</span>
    </div>
  );
}

function BlockError({ label, error, retry }: { label: string; error: string; retry: () => void }) {
  return (
    <div className="callout callout-error block-error" role="alert">
      <p>Could not load {label}: {withCode(error)}</p>
      <button type="button" onClick={retry}>Retry</button>
    </div>
  );
}

export function Bar({ value, label }: { value: number; label: string }) {
  const v = Math.max(0, Math.min(1, value));
  return (
    <div className="bar" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(v * 100)}>
      <div className="bar-fill" style={{ width: `${v * 100}%` }} />
      <span className="bar-text">{pct(v)}</span>
    </div>
  );
}

export function Badge({ status }: { status: Status | string }) {
  return <span className={`badge badge-${String(status).toLowerCase().replace(/[^a-z]/g, "")}`}>{status}</span>;
}

export function Stat({ label, value, sub }: { label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <div className="stat" role="group" aria-label={label}>
      <span className="stat-label">{label}</span>
      <strong className="stat-value">{value}</strong>
      {sub && <span className="stat-sub">{sub}</span>}
    </div>
  );
}

// One time-ordered list of meetings and planned work, so the day reads top to bottom.
function Agenda({ meetings, schedule }: { meetings: Meeting[]; schedule: ScheduleItem[] }) {
  const blocks = [
    ...meetings.map((m) => ({ key: `m${m.start}${m.title}`, start: m.start, end: m.end, kind: "meeting", title: m.title, task: "", project: "", reason: "", carry: false, links: undefined, context: "" })),
    ...schedule.map((s) => ({ key: `t${s.start}${s.task}`, start: s.start, end: s.end, kind: "task", title: s.title, task: s.task, project: s.project ?? "", reason: s.reason, carry: s.carry_over, links: s.links, context: s.task_why ?? "" })),
  ].sort((a, b) => a.start.localeCompare(b.start));
  if (!blocks.length) return <p className="muted small">Nothing planned.</p>;
  return (
    <ol className="agenda">
      {blocks.map((b) => (
        <li key={b.key} className={`block block-${b.kind}`}>
          <span className="time">{hhmm(b.start)}–{hhmm(b.end)}</span>
          <div className="block-body">
            <div className="block-title">
              {b.title}
              {b.project && <span className="tag">{b.project}</span>}
              {b.carry && <span className="tag">carry-over</span>}
            </div>
            {b.reason && <div className="muted small">{keepDates(b.reason)}</div>}
            <TaskWhy text={b.context} />
            <TaskLinks links={b.links} />
            {b.task && <TaskDetails id={b.task} />}
          </div>
        </li>
      ))}
    </ol>
  );
}

// Raw task ids mean nothing to a reader; keep them one click away for commands like `careerbrew work done <id>`.
function TaskDetails({ id }: { id: string }) {
  return <details className="task-details small"><summary>Details</summary>Task id <span className="task-id">{id}</span></details>;
}

// Core text marks commands with `backticks`; show them as code instead of raw backticks.
// Notes and study cards are markdown. Raw HTML is never rendered (react-markdown's default), and links open outside the app.
export function Markdown({ text }: { text: string }) {
  return (
    <div className="page prose">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ a: ({ href, children }) => <a href={href} target="_blank" rel="noreferrer noopener">{children}</a> }}>{text}</ReactMarkdown>
    </div>
  );
}

// Reader reports feed the improvement loop: `careerbrew cards explain-queue` sends the card back for a rewrite.
export function ReportProblem({ id }: { id: string }) {
  const [note, setNote] = useState("");
  const [sent, setSent] = useState("");
  const act = useAct();
  if (sent) return <p className="muted small" role="status">{sent}</p>;
  return (
    <details className="report">
      <summary>Report a problem with this explanation</summary>
      <form className="search" onSubmit={(e) => { e.preventDefault(); void act.run(() => api.flagExplanation(id, note)).then((r) => r && setSent("Thanks: the card will be rewritten in the next card-explainer run.")); }}>
        <input aria-label="What is wrong" placeholder="What is wrong or unclear?" value={note} onChange={(e) => setNote(e.target.value)} />
        <button type="submit" disabled={act.busy}>Report</button>
      </form>
      <ActStatus act={act} label="Sending…" />
    </details>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty">{children}</p>;
}

// ---------- views ----------

// ---------- dashboard ----------

// Each block asks for its own data (`/api/focus/<id>`, issue #142), so a horizon shows as soon as it is ready.
const HORIZONS: { id: string; label: string }[] = [
  { id: "now", label: "Right now" },
  { id: "today", label: "Today" },
  { id: "week", label: "This week" },
  { id: "month", label: "This month" },
  { id: "quarter", label: "This quarter" },
  { id: "year", label: "This year" },
  { id: "company", label: "This company" },
];

function useFocusBlock(id: string) {
  return useData<FocusBlock>(() => api.focusBlock(id), [id], { live: true, key: `/api/focus/${id}` });
}

// Issue #146: the order is what you act on first (VIEW_BLOCKS in order.ts): the update to send, the horizons from
// right now outwards, tomorrow, then the stale sources to check.
export function DashboardView() {
  return (
    <>
      <PmCopyBar />
      <FirstRun />
      <div className="stack" data-block="horizons">
        <p className="muted small">The most efficient thing to do at each horizon, with the numbers behind it.</p>
        <About block="horizons" />
        <div className="grid">
          {HORIZONS.map((h) => <HorizonCard key={h.id} id={h.id} label={h.label} />)}
        </div>
      </div>
      <TomorrowCard />
      <Commitments />
      <StaleSources />
      <TimeSavedCard />
    </>
  );
}

// Issue #147: the first steps for a new user, each ticked from data careerbrew has (`careerbrew checklist`), until all
// are done or the owner hides the list (the `first_run_checklist` setting). While it first loads it shows nothing, so a
// user who finished it never sees it flash. It is not live: the watcher step reads `day/watch.json`, which the change
// version skips (test_watch_idle.py), so the list refreshes when Dashboard opens.
function FirstRun() {
  const q = useData(api.checklist, []);
  const act = useAct();
  const [hidden, setHidden] = useState(false);
  if (hidden || (q.data === null && !q.error) || (q.data && !q.data.show)) return null;
  const hide = async () => {
    if (await act.run(() => api.setConfig("first_run_checklist", "false"))) setHidden(true);
  };
  return (
    <Section title={q.data ? `Getting started (${q.data.done} of ${q.data.total} done)` : "Getting started"} block="first-run" action={<button type="button" className="secondary" disabled={act.busy} onClick={hide}>Hide checklist</button>}>
      <State q={q} label="the getting-started checklist">
        {(d) => (
          <ol className="checklist">
            {d.steps.map((s) => (
              <li key={s.id} data-step={s.id} data-done={s.done ? "yes" : "no"}>
                <span className={`tag ${s.done ? "tag-yes" : ""}`}>{s.done ? "done" : "to do"}</span> <a href={s.view}>{s.title}</a>
                {!s.done && <div className="muted small">{s.why}. Or run <code>{s.command}</code></div>}
              </li>
            ))}
          </ol>
        )}
      </State>
      <ActStatus act={act} label="Hiding…" />
    </Section>
  );
}

const minutes = (m: number) => `${m.toFixed(1)} min`;

// US-86: the minutes careerbrew saved you per week (`careerbrew metrics time-saved`): each action's manual baseline
// minus your time (in this app: from the open to Copy, Send, or Accept; for a CLI run: its run time). The last 4 weeks.
function TimeSavedCard() {
  const q = useData(api.timeSaved, []);
  return (
    <Section title="Time saved" block="time-saved">
      <State q={q} label="the time saved">
        {(d) => {
          const weeks = d.weeks ?? [];  // an older server's answer has no weeks
          if (!weeks.length) return <Empty>No timed runs yet. Copy or send a PM update, copy the standup, or accept a suggestion, and the minutes show here.</Empty>;
          const now = weeks.find((w) => w.week === d.this_week);
          const cells = (w: (typeof weeks)[number]) => Object.values(w.actions);
          return (
            <>
              <p data-this-week>
                <strong>{minutes(now?.minutes_saved ?? 0)}</strong> saved this week, over {now?.runs ?? 0} run{now?.runs === 1 ? "" : "s"}.
              </p>
              <DataTable>
                <thead><tr><th>Week of</th><th>Runs</th><th>Saved</th><th>Your time</th></tr></thead>
                <tbody>
                  {weeks.slice(-4).reverse().map((w) => {
                    const web = cells(w).reduce((n, c) => n + c.runs_web, 0);
                    return (
                      <tr key={w.week} data-week={w.week}>
                        <td>{shortDay(w.start)}</td>
                        <td>{w.runs}</td>
                        <td>{minutes(w.minutes_saved)}</td>
                        <td>{minutes(cells(w).reduce((n, c) => n + c.minutes_spent, 0))} <span className="muted small">({web} of {w.runs} timed in this app; the rest is CLI run time)</span></td>
                      </tr>
                    );
                  })}
                </tbody>
              </DataTable>
              <p className="muted small">{d.measured_baselines} of {Object.keys(d.baselines ?? {}).length} manual times are yours; the rest are estimates. Time one by hand: <code>careerbrew time baseline &lt;action&gt; &lt;minutes&gt;</code></p>
            </>
          );
        }}
      </State>
    </Section>
  );
}

// Issue #268: promises you made in your own messages, tracked as projects until done (`careerbrew work promise list`).
// Overdue first, then by due day (the server's order), 5 at a time. Done and Not a promise close one: it leaves this
// block and tomorrow's handoff. With no open promise the block is not shown.
function Commitments() {
  const q = useData(api.promises, [], { live: true });
  const act = useAct();
  const close = async (name: string, how: "done" | "dropped") => {
    if (await act.run(() => api.closePromise(name, how))) q.reload();
  };
  const open = q.data?.promises ?? [];  // an older server's answer has no promises
  if (!q.error && !open.length) return null;
  return (
    <Section title={q.data ? `Commitments (${open.length})` : "Commitments"} block="commitments" action={<span className="muted small">promises you made, until you close them</span>}>
      <State q={q} label="your commitments">
        {(d) => (
          <Paged id="commitments" rows={d.promises ?? []} size={PAGE_SIZE.commitments} noun="commitments">
            {(rows) => (
              <ul className="rows">
                {rows.map((p) => (
                  <li key={p.name} data-promise={p.name}>
                    {p.url ? <a href={p.url} target="_blank" rel="noreferrer noopener">{p.title}</a> : <strong>{p.title}</strong>}
                    {p.overdue && <span className="tag tag-no">overdue</span>}
                    <div className="muted small">
                      {[p.to && `to ${p.to}`, p.due ? `due ${shortDay(p.due)}` : "no due day", `next: ${d.steps?.[p.next] ?? p.next}`].filter(Boolean).join(" · ")}
                    </div>
                    <div className="quick-add small">
                      <button type="button" className="secondary" aria-label={`Done: ${p.title}`} disabled={act.busy} onClick={() => close(p.name, "done")}>Done</button>
                      <button type="button" className="secondary" aria-label={`Not a promise: ${p.title}`} disabled={act.busy} onClick={() => close(p.name, "dropped")}>Not a promise</button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </Paged>
        )}
      </State>
      <ActStatus act={act} label="Closing…" />
    </Section>
  );
}

// Never checked first, then the stalest (the server's order), 5 at a time.
function StaleSources() {
  const q = useFocusBlock("stale");
  if (q.data === null) return <div data-block="stale-sources"><About block="stale-sources" /><State q={q} label="stale project sources" compact>{() => null}</State></div>;
  if (!q.data.stale?.length) return q.error ? <div data-block="stale-sources"><About block="stale-sources" /><State q={q} label="stale project sources">{() => null}</State></div> : null;
  return (
    <section className="card" aria-label="Stale project sources" data-block="stale-sources">
      <header className="card-head"><h2>Stale project sources ({q.data.stale.length})</h2><span className="muted small">check them, so the ranks above use fresh data</span></header>
      <About block="stale-sources" />
      <State q={q} label="stale project sources">
        {(d) => (
          <Paged id="stale-sources" rows={d.stale ?? []} size={PAGE_SIZE["stale-sources"]} noun="stale sources">
            {(rows) => (
              <ol className="list small">
                {rows.map((s) => (
                  <li key={`${s.project}-${s.kind}-${s.ref}`}><strong>{s.project}</strong> {s.kind} <code>{s.ref}</code> <span className="muted">{s.age_days === null ? "never checked" : `${s.age_days}d ago`} (limit {s.limit}d) · then <code>careerbrew work project check {s.project} {s.ref}</code></span></li>
                ))}
              </ol>
            )}
          </Paged>
        )}
      </State>
    </section>
  );
}

function HorizonItems({ h }: { h: Horizon }) {
  return h.items.length ? (
    <ol className="list">
      {h.items.map((it, i) => (
        <li key={`${h.id}-${i}`}>
          <strong>{keepDates(it.title)}</strong>{it.project && <> <span className="tag">{it.project}</span></>}
          <div className="why small"><span className="muted">Why: </span>{keepDates(it.why)}</div>
          <TaskWhy text={it.task_why} />
          <TaskLinks links={it.links} />
        </li>
      ))}
    </ol>
  ) : <p className="muted">{h.empty}</p>;
}

function HorizonCard({ id, label }: { id: string; label: string }) {
  const q = useFocusBlock(id);
  return (
    <section className="card horizon" aria-label={label} data-horizon={id}>
      <header className="card-head"><h2>{label}</h2></header>
      <State q={q} label={label.toLowerCase()}>
        {(d) => (
          <>
            {d.horizon && <HorizonItems h={d.horizon} />}
            {id === "now" && <p className="muted small foot">As of {hhmm(d.at)}</p>}
            {id === "company" && d.ladder && <p className="muted small foot">Ladder <code>{d.ladder}</code>{d.company_ladder ? " (your company's)" : " (generic)"}</p>}
          </>
        )}
      </State>
    </section>
  );
}

function TomorrowCard() {
  const q = useFocusBlock("tomorrow");
  const t = q.data?.tomorrow;
  return (
    <section className="card" aria-label="Tomorrow" data-tomorrow={t?.day} data-block="tomorrow">
      <header className="card-head"><h2>Tomorrow</h2><a className="small" href="#/today">Today's plan</a></header>
      <About block="tomorrow" />
      <State q={q} label="tomorrow's plan">
        {(d) => d.tomorrow && (
          <>
            <p className="muted small">Proposed plan for {shortDay(d.tomorrow.day)}</p>
            {d.tomorrow.items.length ? (
              <ol className="list">
                {d.tomorrow.items.map((it) => (
                  <li key={`${it.ref}-${it.start}`}>
                    <span className="muted">{it.start}–{it.end}</span> <strong>{it.title}</strong>{it.project && <> <span className="tag">{it.project}</span></>}
                    <TaskLinks links={it.links} />
                  </li>
                ))}
              </ol>
            ) : <p className="muted">{d.tomorrow.empty}</p>}
          </>
        )}
      </State>
    </section>
  );
}

const CALENDAR_SETUP = "Calendar not connected: add one in private/day/calendars.yaml (see docs/integrations.md).";

// The work feed's open asks (act, no later reply of yours), newest first (issue #275), each with its source link.
// The server sends the 50 newest; past that, "Showing 50 of N" and Show all load the rest, up to the
// answer's `limit` (server.ASKS_LIMIT); past it, the terminal command with `--top <total>` lists the oldest ones too.
// The wait and the overdue mark count work hours only (work_start to work_end on work_days), as `work feed show` does.
// Mark personal runs the same `work feed mark` action as the CLI, so the ask leaves evidence, the day log, and the PM update.
// Not an ask and Done close the ask (issue #185); three Not an ask marks from one channel propose a feed rule.
type Mark = "personal" | "work" | "not-ask" | "done" | "ask";
const MARKED: Partial<Record<Mark, { text: string; undo: Mark }>> = {
  personal: { text: "Marked personal", undo: "work" },
  "not-ask": { text: "Marked not an ask", undo: "ask" },
  done: { text: "Marked done elsewhere", undo: "ask" },
};
const MARKED_WHY: Partial<Record<Mark, string>> = {
  personal: "It stays out of evidence, the day log, and the PM update.",
  "not-ask": "It leaves your open asks; three from one channel suggest a rule.",
  done: "It leaves your open asks.",
};

function OpenAsks() {
  // Component scope: how many asks Show all asked the server for; null is the server's default (the 50 newest).
  const [upTo, setUpTo] = useState<number | null>(null);
  const q = useData(() => api.feedAsksUpTo(upTo), [upTo], { live: true, key: upTo ? `/api/feed/asks?limit=${upTo}` : api.feedAsks.key });
  const act = useAct();
  const [marked, setMarked] = useState<{ ask: FeedAsk; value: Mark } | null>(null);
  const mark = async (ask: FeedAsk, value: Mark) => {
    const r = await act.run(async () => {
      const out = await api.runAction("work.feed.mark", { id: ask.id, value });
      if (out.exit !== 0) throw new Error(out.output.trim().replace(/^error: /, "") || `work feed mark stopped with exit ${out.exit}`);
      return out;
    });
    if (r) {
      setMarked(MARKED[value] ? { ask, value } : null);
      q.reload();
    }
  };
  return (
    <Section title={q.data ? `Open asks (${q.data.total})` : "Open asks"} block="open-asks" action={<span className="muted small">newest first; answered, personal, and marked ones are left out</span>}>
      <State q={q} label="open asks">
        {(d) => (
          <>
          {d.closed_today !== undefined && <p className="muted closed-today-count">Closed today: {d.closed_today}</p>}
          {d.asks.length ? (
            <Paged id="open-asks" rows={d.asks} size={PAGE_SIZE["open-asks"]} noun="open asks">
              {(rows) => (
                <ul className="rows">
                  {rows.map((a) => (
                    <li key={a.id} data-ask={a.id}>
                      <span className="tag">{a.source}</span>{" "}
                      {a.url ? <a href={a.url} target="_blank" rel="noreferrer noopener">{a.title}</a> : <strong>{a.title}</strong>}
                      {a.overdue && <span className="tag tag-no">over {d.overdue_hours} work hours</span>}
                      <div className="quick-add small">
                        <span className="muted">waiting {duration(a.work_minutes)} in work hours</span>
                        <button type="button" className="secondary" aria-label={`Not an ask: ${a.title}`} disabled={act.busy} onClick={() => mark(a, "not-ask")}>Not an ask</button>
                        <button type="button" className="secondary" aria-label={`Done: ${a.title}`} disabled={act.busy} onClick={() => mark(a, "done")}>Done</button>
                        <button type="button" className="secondary" aria-label={`Mark personal: ${a.title}`} disabled={act.busy} onClick={() => mark(a, "personal")}>Mark personal</button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </Paged>
          ) : (
            <Empty>{d.events ? "No open asks: you answered everything." : <>No work feed yet: connect a tool with <code>careerbrew connect slack</code> (or pagerduty, outlook), then <code>careerbrew work feed pull</code>.</>}</Empty>
          )}
          {d.total > d.asks.length && (
            <p className="muted small foot">
              Showing {d.asks.length} of {d.total} open asks, newest first.{" "}
              {d.asks.length < Math.min(d.total, d.limit) ? (
                <button type="button" className="secondary" onClick={() => setUpTo(Math.min(d.total, d.limit))}>{d.total > d.limit ? `Show ${d.limit}` : `Show all ${d.total}`}</button>
              ) : (
                <>The {d.total - d.asks.length} oldest are only in your terminal: <code>careerbrew work feed show --show --open --top {d.total}</code></>
              )}
            </p>
          )}
          </>
        )}
      </State>
      {marked && !act.busy && (
        <p className="muted small" role="status">
          {MARKED[marked.value]?.text}: {marked.ask.title}. {MARKED_WHY[marked.value]}{" "}
          <button type="button" className="secondary" onClick={() => mark(marked.ask, MARKED[marked.value]?.undo ?? "ask")}>Undo</button>
        </p>
      )}
      <ActStatus act={act} label="Marking…" />
    </Section>
  );
}

// Active now (issue #198): the ranked projects (`project_priority`), rank 1 first, with the counts `work priority show`
// prints. Move up, Move down, and Remove run the same `work priority set` and `remove` actions as the CLI; Put back
// sets a removed project again at its old rank with its old patterns, offered only while a fresh answer shows the
// ranking the Remove left (a change from the CLI, another tab, or the other view drops it). `onChange` lets a view reload what the rank sorts.
// `hideEmpty` (Today) shows nothing until a project is ranked, so a new user's Today opens on what needs them.
export function ActiveNow({ onChange, hideEmpty }: { onChange?: () => void; hideEmpty?: boolean }) {
  const q = useData(api.priority, [], { live: true });
  const act = useAct();
  // Component scope: the removed row, the ranking before and after its Remove, and whether an answer showed `after`.
  // An answer with the `before` ranking may predate the Remove, so it counts only once `after` was seen; any other
  // ranking is a change made elsewhere and drops Put back.
  const [removed, setRemovedState] = useState<{ row: PriorityRow; before: string; after: string; armed: boolean } | null>(null);
  const sig = (rows: PriorityRow[]) => JSON.stringify(rows.map((r) => [r.name, r.rank, r.match]));
  const setRemoved = (p: PriorityRow | null) => setRemovedState(p && {
    row: p,
    before: sig(q.data?.projects ?? []),
    after: sig((q.data?.projects ?? []).filter((r) => r.name !== p.name).map((r, i) => ({ ...r, rank: i + 1 }))),
    armed: false,
  });
  useEffect(() => {
    if (!removed || !q.data || q.cached) return;
    const same = sig(q.data.projects ?? []) === removed.after;
    if (same && !removed.armed) setRemovedState({ ...removed, armed: true });
    else if (!same && (removed.armed || sig(q.data.projects ?? []) !== removed.before)) setRemovedState(null);
  }, [q.data, q.cached, removed]);
  const run = async (name: "work.priority.set" | "work.priority.remove", values: Record<string, unknown>) => {
    const r = await act.run(async () => {
      const out = await api.runAction(name, values);
      if (out.exit !== 0) throw new Error(out.output.trim().replace(/^error: /, "") || `${name.split(".").join(" ")} stopped with exit ${out.exit}`);
      return out;
    });
    if (r) {
      q.reload();
      onChange?.();
    }
    return Boolean(r);
  };
  const move = (p: PriorityRow, rank: number) => { setRemoved(null); void run("work.priority.set", { project: p.name, rank }); };
  const remove = async (p: PriorityRow) => { if (await run("work.priority.remove", { project: p.name })) setRemoved(p); };
  const putBack = async (p: PriorityRow) => { if (await run("work.priority.set", { project: p.name, rank: p.rank, match: p.match })) setRemoved(null); };
  const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;
  if (hideEmpty && !q.data?.projects?.length && !removed && !act.busy) return null; // also while loading: no skeleton that may vanish
  return (
    <Section title="Active now" block="active-now" action={<a className="small" href="#/actions/work.priority.set">Rank a project</a>}>
      <State q={q} label="your ranked projects">
        {(d) => d.projects?.length ? (
          <ol className="rows active-now">
            {d.projects.map((p, i) => (
              <li key={p.name} data-rank={p.rank}>
                <div className="row-head">
                  <span><span className="tag tag-rank">#{p.rank}</span> <strong>{p.name}</strong></span>
                  <span className="muted small">{plural(p.open_tasks, "open task")} · {plural(p.today, "item")} today</span>
                </div>
                <div className="quick-add small">
                  <button type="button" className="secondary" aria-label={`Move up: ${p.name}`} disabled={act.busy || i === 0} onClick={() => move(p, p.rank - 1)}>↑ Up</button>
                  <button type="button" className="secondary" aria-label={`Move down: ${p.name}`} disabled={act.busy || i === d.projects.length - 1} onClick={() => move(p, p.rank + 1)}>↓ Down</button>
                  <button type="button" className="secondary" aria-label={`Remove from ranking: ${p.name}`} disabled={act.busy} onClick={() => remove(p)}>Remove</button>
                </div>
              </li>
            ))}
          </ol>
        ) : <Empty>No ranked projects yet. Rank one, and the PM update, the plan, and Next step follow it: <code>careerbrew work priority set &lt;project&gt; --match &apos;&lt;glob&gt;&apos;</code></Empty>}
      </State>
      {removed && !act.busy && (
        <p className="muted small" role="status">
          Removed {removed.row.name} from the ranking. Its project and tasks stay.{" "}
          <button type="button" className="secondary" aria-label={`Put back: ${removed.row.name}`} onClick={() => putBack(removed.row)}>Put back</button>
        </p>
      )}
      <ActStatus act={act} label="Saving the ranking…" />
    </Section>
  );
}

// Issue #146: the update to send, the next step, the asks that need you, the plan, then the log (VIEW_BLOCKS in order.ts).
export function TodayView() {
  const health = useFeedHealth();
  const q = useData(api.today, [], { live: true });
  const day = useDayLog();
  return (
    <div className="stack">
      <PmCopyBar />
      <FeedHealthNotice data={health.data} />
      <ActiveNow onChange={q.reload} hideEmpty />
      <PmUpdate log={day.log} />
      <Section title="Next step" className="hero" block="next-step">
        <State q={q} label="the next step">
          {(d) => (
            <>
              <p className="next">{d.next.title ?? d.next.text}</p>
              {d.next.title && <p className="next-how">{d.next.plain ?? d.next.text}</p>}
              {d.next.reason && <p className="muted small">Why: {keepDates(d.next.reason)}</p>}
              <TaskWhy text={d.next.task_why} />
              <TaskLinks links={d.next.links} />
            </>
          )}
        </State>
      </Section>
      <OpenAsks />
      <InboxBlock type="question" items={day.items} onDone={day.onDone} />
      <InboxBlock type="proposal" items={day.items} onDone={day.onDone} />
      <div className="stack" data-block="plan">
      <About block="plan" />
      <State q={q} label="today's plan">
        {(d) => (
          <div className="split">
            <Section title={`Plan for ${shortDay(d.plan.day)}`}>
              <Agenda meetings={d.plan.meetings} schedule={d.plan.schedule} />
              {d.plan.unplaced.length > 0 && <p className="muted small foot">Not today: {(d.plan.unplaced_titles ?? d.plan.unplaced).join(", ")}</p>}
              {d.plan.calendar_connected === false && <p className="callout callout-warn">{CALENDAR_SETUP}</p>}
              {d.plan.calendar_errors.map((e) => <p key={e} className="callout callout-error">Calendar: {e}</p>)}
            </Section>
            <div className="stack">
              <div className="stats">
                {d.plan.calendar_connected !== false && <>
                  <Stat label="Meetings" value={d.plan.meetings.length} />
                  <Stat label="Free" value={duration(d.plan.free_minutes)} />
                  <Stat label="Focus" value={duration(d.plan.focus_minutes)} />
                </>}
                <Stat label="Estimates" value={`×${d.plan.calibration}`} sub="your usual overrun" />
              </div>
              <Section title="Results of the day">
                {d.results.length ? (
                  <Paged id="results" rows={[...d.results].sort((a, b) => b.at.localeCompare(a.at))} size={PAGE_SIZE.results} noun="results">
                    {(rows) => <ul className="rows">{rows.map((r) => <li key={r.id}><span className="tag">{r.kind}</span> {r.title}<SourceAge row={r} health={health.data} /></li>)}</ul>}
                  </Paged>
                ) : <Empty>Nothing logged yet today.</Empty>}
              </Section>
            </div>
          </div>
        )}
      </State>
      </div>
      <DayLog log={day.log} />
      <EarlierDays log={day.log} />
    </div>
  );
}

const WEEK_DAYS = [5, 7, 14];

export function WeekView() {
  // The day count is a filter of the view: it lives in the hash (`#/week?days=14`), so a reload keeps it (issue #146).
  const [state, setState] = useHashState();
  const days = WEEK_DAYS.includes(Number(state.days)) ? Number(state.days) : 7;
  const setDays = (n: number) => setState({ days: n === 7 ? null : String(n) });
  const q = useData(() => api.week(days), [days], { live: true, key: `/api/week?days=${days}` });
  const [title, setTitle] = useState("");
  const [hours, setHours] = useState(1);
  const [due, setDue] = useState("");
  const act = useAct();
  const add = async () => {
    if (!title.trim()) return;
    const r = await act.run(() => api.addTask({ title, hours, due: due || undefined }));
    if (!r) return;
    setTitle("");
    q.reload();
  };
  return (
    <div className="stack">
      <Section title="Plan ahead" block="plan-ahead" action={
        <label className="inline muted small">Days <select aria-label="Days to show" value={days} onChange={(e) => setDays(Number(e.target.value))}>{WEEK_DAYS.map((n) => <option key={n} value={n}>{n}</option>)}</select></label>
      }>
        <form className="quick-add" onSubmit={(e) => { e.preventDefault(); void add(); }}>
          <input aria-label="New task" placeholder="Add a task to plan…" value={title} onChange={(e) => setTitle(e.target.value)} />
          <label className="field">Hours <input aria-label="Hours" type="number" min={0.25} step={0.25} value={hours} onChange={(e) => setHours(Number(e.target.value))} /></label>
          <label className="field">Due <input aria-label="Due date" type="date" value={due} onChange={(e) => setDue(e.target.value)} /></label>
          <button type="submit" className="primary" disabled={act.busy || !title.trim()}>Add task</button>
        </form>
        <ActStatus act={act} label="Adding…" />
      </Section>
      <div className="stack" data-block="days">
      <About block="days" />
      <State q={q} label="the week">
        {(d) => {
          const last = d.days[d.days.length - 1];
          const unplaced = last?.unplaced_titles ?? last?.unplaced ?? [];
          const tooBig = last?.too_big ?? [];
          return (
            <>
              <div className="week" style={{ "--cols": Math.min(d.days.length, 7) } as CSSProperties}>
                {d.days.map((day) => (
                  <article key={day.day} className={`day${day.workday ? "" : " day-off"}`} aria-label={`${day.weekday} ${day.day}`}>
                    <h3>{day.weekday} <span className="muted">{shortDay(day.day).split(" ").slice(1).join(" ")}</span></h3>
                    <p className="muted small">{day.workday && day.calendar_connected === false ? "Calendar not connected" : day.workday ? <><span className="nowrap">{day.meetings.length} meeting{day.meetings.length === 1 ? "" : "s"}</span> · <span className="nowrap">{duration(day.free_minutes)} free</span></> : "Not a workday"}</p>
                    <Agenda meetings={day.meetings} schedule={day.schedule} />
                  </article>
                ))}
              </div>
              {d.days[0]?.calendar_connected === false && <p className="callout callout-warn">{CALENDAR_SETUP}</p>}
              {tooBig.length > 0 && <p className="callout callout-warn">Too big for one day: {tooBig.map((b) => `${b.title ?? b.task} (${duration(b.minutes)})`).join(", ")}. A day here has at most {duration(last?.day_focus_minutes ?? 0)} of focus time: split {tooBig.length === 1 ? "it" : "each"} into tasks of a day or less.</p>}
              {unplaced.length > 0 && <p className="callout callout-warn">No room in these {d.days.length} days for: {unplaced.join(", ")}</p>}
            </>
          );
        }}
      </State>
      </div>
    </div>
  );
}

function stageTone(stage: string | null): string {
  if (!stage) return "";
  if (/^no\b/i.test(stage)) return " tag-no";
  if (/hired|offer/i.test(stage)) return " tag-yes";
  return "";
}

const TIERS = ["dream", "target", "practice"] as const;

// Job hunt's suggestions you have not chosen yet: dream first, then target, then practice, each tier in the
// server's order (issue #146: one row per scanned company, so the list pages).
function openSuggestions(s: Jobs["strategy"]): { tier: (typeof TIERS)[number]; row: TierRow }[] {
  const chosen = Object.values(s.tiers ?? {}).flat();
  return TIERS.flatMap((tier) => (s.suggested?.[tier] ?? []).filter((r) => !chosen.some((c) => c.posting_id === r.posting_id)).map((row) => ({ tier, row })));
}

export function JobsView() {
  const q = useData(api.jobs);
  const act = useAct();
  const pick = (fn: () => Promise<unknown>) => void act.run(fn).then((r) => r && q.reload());
  return (
    <State q={q} label="your job search">
      {(d) => (
        <div className="stack">
          <ActStatus act={act} label="Saving your choice…" />
          <Section title="Season strategy" block="strategy" action={d.strategy.phase ? <div className="readiness"><span className="muted small">Readiness</span><Bar value={d.strategy.readiness ?? 0} label="Readiness" /></div> : undefined}>
            {d.strategy.phase ? (
              <>
                <ol className="phases">
                  {(d.strategy.steps ?? []).map((s) => {
                    const now = s.startsWith("NOW");
                    const text = s.replace(/^NOW\s*→\s*/, "");
                    const [head, ...rest] = text.split(":");
                    return (
                      <li key={s} className={now ? "phase phase-now" : "phase"} aria-current={now ? "step" : undefined}>
                        <strong>{head}</strong>{now && <span className="badge badge-keep">now</span>}
                        {rest.length > 0 && <div className="small">{withCode(rest.join(":").trim())}</div>}
                      </li>
                    );
                  })}
                </ol>
                {(d.strategy.skill_gaps ?? []).length > 0 && (
                  <p className="chips"><span className="muted small">Skill gaps</span>{(d.strategy.skill_gaps ?? []).map(([s]) => <span key={s} className="tag">{s}</span>)}</p>
                )}
              </>
            ) : <Empty>No strategy yet: run <code>careerbrew jobs strategy</code>.</Empty>}
          </Section>
          {d.strategy.tiers && (
            <div className="stack" data-block="tiers">
              <About block="tiers" />
              <div className="columns">
              {(["dream", "target", "practice"] as const).map((tier) => (
                <Section key={tier} title={`${tier[0].toUpperCase()}${tier.slice(1)} companies`} action={<span className="count">{d.strategy.tiers![tier].length}</span>}>
                  {d.strategy.tiers![tier].length ? (
                    <ul className="tiles">{d.strategy.tiers![tier].map((r) => (
                      <li key={r.company + r.title} className="tile">
                        <div className="row-head"><strong>{r.url ? <a href={r.url} rel="noreferrer noopener" target="_blank">{r.company}</a> : r.company}</strong>{r.posting_id && <button type="button" className="small" disabled={act.busy} onClick={() => pick(() => api.unchooseJob(r.posting_id!))}>Remove</button>}</div>
                        <div>{r.title}</div>
                        <div className="muted small">Want {r.want} · EV {r.ev}</div>
                      </li>
                    ))}</ul>
                  ) : <Empty>None chosen. Pick from the suggestions below.</Empty>}
                </Section>
              ))}
              </div>
            </div>
          )}
          {/* Your 1-5 rating sets a job's Want in the strategy (rating x 20); the postings table shows both (US-75). */}
          {d.strategy.suggested && Object.values(d.strategy.suggested).some((rows) => rows.length > 0) && (
            <Section title="Suggestions: you choose" block="job-suggestions">
              <p className="muted small">careerbrew only suggests. A job enters your plan when you choose it and its tier.</p>
              <Paged id="job-suggestions" rows={openSuggestions(d.strategy)} size={PAGE_SIZE["job-suggestions"]} noun="job suggestions" facet={(s) => s.tier}>
                {(rows) => (
                  <DataTable>
                    <thead><tr><th>Suggested</th><th>Company</th><th>Role</th><th className="num">Want</th><th>Choose as</th></tr></thead>
                    <tbody>{rows.map(({ tier, row: r }) => (
                      <tr key={r.posting_id}><td className="muted">{tier}</td><td>{r.company}</td><td>{r.url ? <a href={r.url} rel="noreferrer noopener" target="_blank">{r.title}</a> : r.title}</td><td className="num">{r.want}{rated(d.postings, r.posting_id) && <span className="muted small"> (you rated {rated(d.postings, r.posting_id)})</span>}</td>
                        <td><div className="buttons">{TIERS.map((t) => <button key={t} type="button" aria-label={`Choose ${r.company} as ${t}`} disabled={act.busy} onClick={() => pick(() => api.chooseJob(r.posting_id!, t))}>{t}</button>)}</div></td></tr>
                    ))}</tbody>
                  </DataTable>
                )}
              </Paged>
            </Section>
          )}
          {d.board.length > 0 && (
            <Section title={`Jobs board from Notion (${d.board.length})`} block="board">
              <Paged id="board" rows={[...d.board].sort((a, b) => (b.want ?? 0) - (a.want ?? 0))} size={PAGE_SIZE.board} noun="jobs on the board">
                {(rows) => (
                  <DataTable>
                    <thead><tr><th className="num">Want</th><th>Company</th><th>Role</th><th>Stage</th><th>Where</th><th>Why not</th></tr></thead>
                    <tbody>
                      {rows.map((j) => (
                        <tr key={j.id}><td className="num">{j.want ?? ""}</td><td>{j.url ? <a href={j.url} rel="noreferrer noopener" target="_blank">{j.company}</a> : j.company}</td><td>{j.role}</td><td>{j.stage && <span className={`tag${stageTone(j.stage)}`}>{j.stage}</span>}</td><td>{[j.remote, ...(j.location ?? [])].filter(Boolean).join(", ")}</td><td className="muted">{j.reason_no ?? ""}</td></tr>
                      ))}
                    </tbody>
                  </DataTable>
                )}
              </Paged>
            </Section>
          )}
          <Section title={`Postings that pass your criteria (${d.postings.length})`} block="postings">
            <Paged id="postings" rows={d.postings} size={PAGE_SIZE.postings} noun="postings">
              {(rows) => (
                <DataTable>
                  <thead><tr><th className="num">Want</th><th className="num">Your rating</th><th>Company</th><th>Role</th><th>Region</th><th>Source</th><th>Loop</th></tr></thead>
                  <tbody>
                    {rows.map((p) => (
                      <tr key={p.id}><td className="num">{p.want}</td><td className="num muted">{p.rating ? `${p.rating}/5` : "—"}</td><td>{p.company}</td><td>{p.url ? <a href={p.url} rel="noreferrer noopener" target="_blank">{p.title}</a> : p.title}</td><td>{p.region || p.location}</td><td className="muted">{p.source}</td><td><a href={`#/interviews/${encodeURIComponent(p.company)}`}>{p.has_loop ? "sourced" : "default"}</a></td></tr>
                    ))}
                  </tbody>
                </DataTable>
              )}
            </Paged>
            {Object.keys(d.rejected).length > 0 && <p className="muted small foot">Filtered out: {Object.entries(d.rejected).map(([k, v]) => `${k} ${v}`).join(", ")}</p>}
          </Section>
        </div>
      )}
    </State>
  );
}

// Your own 1-5 rating of a posting ("5/5"), or "" when you have not rated it.
function rated(postings: { id: string; rating: number | null }[], id?: string): string {
  const r = postings.find((p) => p.id === id)?.rating;
  return r ? `${r}/5` : "";
}

export function InterviewsView({ company }: { company?: string }) {
  const list = useData(() => (company ? Promise.resolve(null) : api.interviews()), [company], { key: company ? null : api.interviews.key });
  const detail = useData(() => (company ? api.interview(company) : Promise.resolve(null)), [company], { key: company ? `/api/interviews/${encodeURIComponent(company)}` : null });
  const [name, setName] = useState("");

  if (company) {
    return (
      <div className="stack">
        <a className="back" href="#/interviews">← All companies</a>
        <State q={detail} label={`the ${company} loop`}>
          {(info) => info && (
            <div className="split">
              <Section title={`Interview loop: ${info.company}`}>
                {!info.known && <p className="callout">No sourced loop yet; this is the shared default. Add one with the careerbrew-browser-research skill.</p>}
                {info.stale && <p className="callout callout-error">Process changed on {keepDates(info.stale.at)} ({info.stale.reason}). Re-read this loop before your interview.</p>}
                <ol className="stepper">
                  {info.loop.map((s) => (
                    <li key={s.id}>
                      <h3>{s.name ?? s.id} {s.typical_minutes && <span className="tag">{/^\d/.test(s.typical_minutes) ? `~${s.typical_minutes} min` : s.typical_minutes}</span>}</h3>
                      <p>{s.description}</p>
                      {s.company_notes && <p className="note"><em>At {info.company}:</em> {s.company_notes}</p>}
                      {((s.tips ?? []).length > 0 || (s.traps ?? []).length > 0) && (
                        <details><summary>Tips and traps</summary>
                          <ul className="list small">{(s.tips ?? []).map((t) => <li key={t.text}>Tip: {t.text}</li>)}{(s.traps ?? []).map((t) => <li key={t.text}>Avoid: {t.text}</li>)}</ul>
                        </details>
                      )}
                    </li>
                  ))}
                </ol>
              </Section>
              <div className="stack">
                <Section title="AI tools in this loop">
                  {info.ai_policy && <p>AI tools: <Badge status={info.ai_policy.allowed} /> {info.ai_policy.summary} <span className="muted small">(checked {keepDates(info.ai_policy.checked)})</span></p>}
                  {info.ai_shared && <><p>AI tools: no sourced rule for {info.company} yet. Ask the recruiter. What sourced companies share:</p><ul className="list small">{info.ai_shared.map((p) => <li key={p.text}>{p.text}</li>)}</ul></>}
                </Section>
                {info.values.length > 0 && (
                  <Section title="Values">
                    <ul className="list small">{info.values.map((v) => <li key={v}>{v}</li>)}</ul>
                  </Section>
                )}
                <section className="card" aria-label="Sources">
                  <details><summary>Sources ({Object.keys(info.source_list).length})</summary>
                    <ul className="list small">{Object.entries(info.source_list).map(([id, s]) => <li key={id}><a href={s.url} rel="noreferrer noopener" target="_blank">{s.title}</a> <span className="muted nowrap">{keepDates(s.checked)}</span></li>)}</ul>
                  </details>
                </section>
              </div>
            </div>
          )}
        </State>
      </div>
    );
  }

  return (
    <State q={list} label="companies">
      {(d) => d && (() => {
        const needle = name.trim().toLowerCase();
        const shown = d.companies.filter((c) => c.name.toLowerCase().includes(needle));
        return (
          <div className="stack">
          <div className="split split-main">
            <Section title="Companies" block="companies" action={<span className="count">{shown.length}</span>}>
              <form className="search" role="search" onSubmit={(e) => { e.preventDefault(); if (name.trim()) window.location.hash = `#/interviews/${encodeURIComponent(name.trim())}`; }}>
                <input aria-label="Any company" placeholder="Filter, or type any company, e.g. Stripe" value={name} onChange={(e) => setName(e.target.value)} />
                <button type="submit" disabled={!name.trim()}>Open loop</button>
              </form>
              {shown.length === 0 && <Empty>No sourced loop for “{name.trim()}”. Open loop gives the shared default with shared AI guidance.</Empty>}
              {shown.length > 0 && (
                <DataTable>
                  <thead><tr><th>Company</th><th className="num">Steps</th><th>AI tools</th><th>Checked</th><th className="num">Prep</th></tr></thead>
                  <tbody>{shown.map((c) => (
                    <tr key={c.id}><td><a href={`#/interviews/${encodeURIComponent(c.name)}`}>{c.name}</a></td><td className="num">{c.steps.length}</td><td><Badge status={c.ai} /></td><td className="muted">{keepDates(c.checked)}</td><td className="num">{pct(c.prep_score)}</td></tr>
                  ))}</tbody>
                </DataTable>
              )}
            </Section>
            <Section title={`Shared patterns across ${d.patterns.companies} companies`} block="patterns">
              <p className="small muted">Typical order</p>
              <ol className="flow">{d.patterns.typical_order.map((s) => <li key={s}>{s}</li>)}</ol>
              <div className="shares">{Object.entries(d.patterns.step_share).map(([s, v]) => <div key={s} className="share"><span>{s}</span><Bar value={v} label={`${s} share`} /></div>)}</div>
              <p className="chips"><span className="muted small">AI tools allowed</span>{Object.entries(d.patterns.ai_policy).map(([k, v]) => <span key={k}><Badge status={k} /> {v}</span>)}</p>
              <ul className="list small">{d.patterns.ai_shared_patterns.map((p) => <li key={p.text}>{p.text}</li>)}</ul>
            </Section>
          </div>
          <VoicePractice />
          </div>
        );
      })()}
    </State>
  );
}

// Issue #172: one section per active project, so the owner (and an agent's routine, from the same list) sees what
// each project's sources are, when one was last read, and the latest change noted on it. Stale sources come first.
const WATCHED_SOURCES = 5;

// Issue #276: a prep project is done only when its four gates pass; each open gate shows the command that closes it.
function PrepGates({ r }: { r: Prep }) {
  return (
    <div className="prep-gates">
      <p className="small"><strong>{r.ready ? "Ready" : "Not ready"}</strong>: {r.done} of {r.total} gates pass{r.event_on && <> · event {keepDates(r.event_on)}</>}</p>
      <ul className="rows" aria-label="Readiness gates">
        {r.gates.map((g) => (
          <li key={g.gate}>
            <div className="row-head">
              <span><span className="tag">{g.gate}</span> {g.text}</span>
              <span className="muted small">{g.done ? "done" : <strong>open</strong>}</span>
            </div>
            {(g.note || g.fix) && <div className="muted small">{g.note}{g.note && g.fix && " · "}{g.fix && <code className="wrap-anywhere">{g.fix}</code>}</div>}
          </li>
        ))}
      </ul>
    </div>
  );
}

function WatchedCard({ p }: { p: WatchedProject }) {
  const sources = [...p.sources].sort((a, b) => Number(b.stale) - Number(a.stale));
  const row = (s: WatchedProject["sources"][number]) => (
    <li key={`${s.kind}-${s.ref}`}>
      <div className="row-head">
        <span><span className="tag">{s.kind}</span> <span className="wrap-anywhere">{s.ref}</span></span>
        <span className="muted small">{s.reference ? "reference" : s.stale ? <strong>stale</strong> : "fresh"} · {s.checked_at ? keepDates(localDate(s.checked_at)) : "never read"}</span>
      </div>
      {s.status && <div className="muted small">{s.status}</div>}
    </li>
  );
  const u = p.latest_update;
  return (
    <section className="watched-project" aria-label={p.name}>
      <h3>{p.name}{p.title && <> · {p.title}</>}{p.group && <> <span className="tag">{p.group}</span></>}</h3>
      <p className="muted small">
        {p.sources.length} source{p.sources.length === 1 ? "" : "s"}{p.stale ? ` · ${p.stale} stale` : ""} · last checked {p.last_checked_at ? keepDates(localDate(p.last_checked_at)) : "never"}
      </p>
      {p.prep && <PrepGates r={p.prep} />}
      <p className="small">{u ? <>Latest change ({u.kind}, {keepDates(localDate(u.at))}){u.text ? <>: {u.text}</> : ""}</> : <span className="muted">No change noted yet.</span>}</p>
      {sources.length ? (
        <>
          <ul className="rows">{sources.slice(0, WATCHED_SOURCES).map(row)}</ul>
          {sources.length > WATCHED_SOURCES && (
            <details><summary className="small">Show {sources.length - WATCHED_SOURCES} more source{sources.length - WATCHED_SOURCES === 1 ? "" : "s"}</summary><ul className="rows">{sources.slice(WATCHED_SOURCES).map(row)}</ul></details>
          )}
        </>
      ) : <p className="muted small">No sources yet.</p>}
    </section>
  );
}

export function WorkView() {
  const q = useData(api.work);
  return (
    <State q={q} label="your work">
      {(d) => (
        <div className="grid">
          <Section title={`Promotion case: ${d.promo.level ?? ""}`} block="promotion">
            {d.promo.error ? <p className="callout callout-error">{d.promo.error}</p> : (
              <>
                <p className="big-number">{pct(d.promo.overall)} <span className="muted small">covered on <code>{d.promo.ladder}</code></span></p>
                <div className="shares">{d.promo.competencies.map((c) => <div key={c.id} className="share"><span>{c.name}</span><Bar value={c.coverage} label={`${c.name} coverage`} /></div>)}</div>
              </>
            )}
          </Section>
          <Section title="What is missing (highest priority first)" block="gaps">
            {d.gaps.length ? (
              <ol className="rows">{d.gaps.map((g) => <li key={g.id}><div className="row-head"><strong>{g.name}</strong><span className="muted small">{pct(g.coverage)} · priority {g.priority}</span></div><div className="muted small">{g.how_to_close}</div></li>)}</ol>
            ) : <Empty>No gaps.</Empty>}
          </Section>
          <Section title="Tasks by priority" block="tasks">
            {d.tasks.length ? (
              <Paged id="tasks" rows={d.tasks} size={PAGE_SIZE.tasks} noun="tasks">
                {(rows) => <ol className="rows">{rows.map((t) => <li key={t.id}><div className="row-head"><span>{t.title}{t.project && <> <span className="tag">{t.project}</span></>}</span><span className="muted small meta-stack"><span>score {t.score}</span> <span>{t.estimate_h}h{t.due ? <> · due {keepDates(t.due)}</> : ""}</span></span></div><TaskDetails id={t.id} /></li>)}</ol>}
              </Paged>
            ) : <Empty>No open tasks.</Empty>}
          </Section>
          <Section title="Watch list" block="watched">
            {d.watched?.length ? (
              <>
              <Paged id="watched" rows={d.watched} size={PAGE_SIZE.watched} noun="watched projects">
                {(rows) => <div className="watched">{rows.map((p) => <WatchedCard key={p.name} p={p} />)}</div>}
              </Paged>
              <p className="muted small">Note a change: <code>careerbrew work project update &lt;name&gt; &lt;company|project|team|other&gt; &quot;&lt;one line&gt;&quot;</code> · add a source: <code>careerbrew work project source &lt;name&gt; &lt;kind&gt; &lt;ref&gt;</code></p>
              </>
            ) : <Empty>No active projects: <code>careerbrew work project add &lt;name&gt;</code>. An agent's routine reads the same list: <code>careerbrew work project list --active --json</code>.</Empty>}
          </Section>
          <ActiveNow onChange={q.reload} />
          <AgentActivity />
          <Section title="Projects" block="projects" action={<a className="small" href="#/actions/work.project.import">Import a sheet</a>}>
            {d.projects?.length ? (
              <Paged id="projects" rows={d.projects} size={PAGE_SIZE.projects} noun="projects">
              {(rows) => <ol className="rows">{rows.map((p) => (
                <li key={p.name} data-depth={p.depth} style={{ paddingLeft: `${Math.min(p.depth, 4) * 16}px` }}>
                  <div className="row-head">
                    <span>{p.depth > 0 && <span className="muted" aria-hidden="true">↳ </span>}{p.rank != null && <><span className="tag tag-rank" aria-label={`rank ${p.rank}`}>#{p.rank}</span> </>}<strong>{p.name}</strong>{p.title && <> · {p.title}</>}{p.group && <> <span className="tag">{p.group}</span></>}{p.archived && <> <span className="tag">archived</span></>}</span>
                    <span className="muted small">{p.sources} source{p.sources === 1 ? "" : "s"}{p.stale ? ` · ${p.stale} stale` : ""}</span>
                  </div>
                  {(p.parent || p.links.length > 0) && <div className="muted small">{p.parent && `part of ${p.parent}`}{p.parent && p.links.length > 0 && " · "}{p.links.length > 0 && `linked to ${p.links.join(", ")}`}</div>}
                </li>
              ))}</ol>}
              </Paged>
            ) : <Empty>No projects yet: <code>careerbrew work project add &lt;name&gt;</code>, or import a team's goal list with <code>careerbrew work project import &lt;file.csv | Google Sheets link&gt;</code>.</Empty>}
          </Section>
          <Section title="Recent evidence" block="evidence">
            {d.evidence.length ? (
              <Paged id="evidence" rows={d.evidence} size={PAGE_SIZE.evidence} noun="evidence">
                {(rows) => <ul className="rows">{rows.map((e) => <li key={e.id}><div className="row-head"><span><span className="tag">{e.kind}</span> {e.title}</span><span className="muted small">{e.at ? keepDates(localDate(e.at)) : ""}</span></div></li>)}</ul>}
              </Paged>
            ) : <Empty>No evidence yet: <code>careerbrew work collect</code>.</Empty>}
          </Section>
        </div>
      )}
    </State>
  );
}

function MetricRow({ m }: { m: Metric }) {
  const cmp = m.higher_is_better ? ["≥", "<"] : ["≤", ">"];
  return (
    <tr>
      <td><Badge status={m.status} /></td>
      <td><code>{m.metric}</code><div className="muted small">{withCode(m.question)}</div></td>
      <td className="num">{m.value ?? "—"}</td>
      <td className="muted small nowrap">keep {cmp[0]} {m.success} · pivot {cmp[1]} {m.pivot}</td>
      <td className="small">{withCode(m.action)}</td>
    </tr>
  );
}

const STATUS_ORDER: { status: Status; label: string }[] = [
  { status: "KEEP", label: "keep" },
  { status: "WATCH", label: "watch" },
  { status: "PIVOT", label: "pivot" },
  { status: "no-data", label: "no data" },
];

export function ReviewView() {
  const q = useData(api.review);
  return (
    <State q={q} label="metrics">
      {(d) => {
        const total = d.metrics.length;
        const count = (s: Status) => d.metrics.filter((m) => m.status === s).length;
        // What needs action comes first: PIVOT, then WATCH, then KEEP, then no data.
        const rank = (s: Status) => { const i = ["PIVOT", "WATCH", "KEEP"].indexOf(s); return i < 0 ? 3 : i; };
        return (
          <Section title="Outcome metrics" block="metrics">
            <div className="summary">
              <p className="big-number">{count("KEEP")} <span className="muted small">of {total} metrics in KEEP</span></p>
              <div className="segments" aria-hidden="true">
                {STATUS_ORDER.map(({ status }) => count(status) > 0 && <span key={status} className={`seg seg-${status.toLowerCase().replace(/[^a-z]/g, "")}`} style={{ flexGrow: count(status) }} />)}
              </div>
              <p className="chips small">{STATUS_ORDER.map(({ status, label }) => <span key={status}><span className={`dot dot-${status.toLowerCase().replace(/[^a-z]/g, "")}`} />{count(status)} {label}</span>)}</p>
            </div>
            <DataTable>
              <thead><tr><th>Status</th><th>Metric</th><th className="num">Value</th><th>Thresholds</th><th>Pivot action</th></tr></thead>
              <tbody>{[...d.metrics].sort((a, b) => rank(a.status) - rank(b.status)).map((m) => <MetricRow key={m.metric} m={m} />)}</tbody>
            </DataTable>
          </Section>
        );
      }}
    </State>
  );
}

const WATCH_ACTIONS: Record<string, { action: "on" | "off" | "pause" | "resume"; label: string }[]> = {
  on: [{ action: "pause", label: "Pause" }, { action: "off", label: "Turn off" }],
  paused: [{ action: "resume", label: "Resume" }, { action: "off", label: "Turn off" }],
  off: [{ action: "on", label: "Turn on" }],
};

export function SetupView() {
  const q = useData(api.setup);
  const ov = useData(api.overview);
  const act = useAct();
  const toggle = async (action: "on" | "off" | "pause" | "resume") => {
    if (await act.run(() => api.watch(action))) ov.reload();
  };
  return (
    <div className="grid">
      {/* Issue #146: what you act on first: the watcher, connectors that need setup, settings; then the facts. */}
      <State q={ov} label="the day watcher">
        {(o) => (
          <Section title="Day watcher" block="watcher" action={<span className={`badge badge-${o.watcher.state === "on" ? "keep" : o.watcher.state === "paused" ? "watch" : "nodata"}`}>{o.watcher.state}</span>}>
            <p className="muted small">Running: {String(o.watcher.running)} · code reloads: {o.watcher.reloads}</p>
            <div className="buttons">
              {(WATCH_ACTIONS[o.watcher.state] ?? WATCH_ACTIONS.off).map((a) => <button key={a.action} type="button" disabled={act.busy} onClick={() => void toggle(a.action)}>{a.label}</button>)}
            </div>
            <ActStatus act={act} />
          </Section>
        )}
      </State>
      <State q={q} label="this machine's setup">
        {(d) => (
          <>
            <Section title="Connectors" block="connectors">
              {d.connectors.length ? (
                <ul className="rows">{d.connectors.map((c) => (
                  <li key={`${c.origin}-${c.kind}`}>
                    <div className="row-head"><span>{c.kind} {c.ready ? <span className="tag">ready</span> : <span className="tag">needs setup</span>}</span><span className="muted small">{c.origin} · secret: {c.secret ?? "none"}</span></div>
                    {!c.ready && <ol className="small">{c.steps.map((s) => <li key={s}>{s}</li>)}</ol>}
                  </li>
                ))}</ul>
              ) : <Empty>None connected.</Empty>}
            </Section>
            <SettingsEditor />
            <Section title="This machine" block="machine">
              <dl className="props"><dt>Role</dt><dd><strong>{d.role}</strong></dd><dt>Plugins</dt><dd>{d.plugins.length}</dd><dt>Connectors</dt><dd>{d.connectors.length}</dd></dl>
            </Section>
            <Section title="Plugins" block="plugins">
              {d.plugins.length ? (
                <ul className="rows">{d.plugins.map((p) => <li key={p.name}><div className="row-head"><strong>{p.name}</strong><span className="muted small">{p.version}{p.extends ? ` · extends ${p.extends}` : ""}</span></div>{p.provides.length > 0 && <div className="muted small">{p.provides.join(", ")}</div>}</li>)}</ul>
              ) : <Empty>No plugins: <code>careerbrew plugin new "Company"</code>.</Empty>}
            </Section>
            <section className="card" aria-label="Policy" data-block="policy">
              <About block="policy" />
              <details><summary>Merged policy</summary><pre className="small code">{JSON.stringify(d.policy, null, 2)}</pre></details>
            </section>
          </>
        )}
      </State>
    </div>
  );
}

// ---------- practice (flashcards) ----------

const GRADES: { rating: Rating; label: string; key: string }[] = [
  { rating: "again", label: "Again", key: "1" },
  { rating: "hard", label: "Hard", key: "2" },
  { rating: "good", label: "Good", key: "3" },
  { rating: "easy", label: "Easy", key: "4" },
];

export function PracticeView() {
  const [tick, setTick] = useState(0);
  const q = useData(api.cards, [tick]);
  const [shown, setShown] = useState(false);
  const [started, setStarted] = useState(() => Date.now());
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const card = q.data?.queue[0];

  const grade = async (rating: Rating) => {
    if (!card || busy) return; // one grade per card, even on a double key press
    setBusy(true);
    setErr(null);
    try {
      const r = await api.reviewCard(card.id, rating, Math.round((Date.now() - started) / 1000));
      setNote(r.review.leech ? "That card is a leech: it is suspended until you rewrite it." : `Next review ${friendlyDay(r.review.due.slice(0, 10))}.`);
      setShown(false);
      setStarted(Date.now());
      setTick((t) => t + 1);
    } catch (e) {
      setErr(explain(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (!shown && (e.key === " " || e.key === "Enter")) { e.preventDefault(); setShown(true); return; }
      const g = GRADES.find((x) => x.key === e.key);
      if (shown && g) void grade(g.rating);
    };
    window.addEventListener("keydown", on);
    return () => window.removeEventListener("keydown", on);
  });

  return (
    <State q={q} label="your cards">
      {(d) => (
        <div className="split split-main">
          <Section title={card ? `${card.mode === "new" ? "New card" : "Review"} · ${card.topic}` : "All done for now"} className="study" block="card" action={card ? <span className="count">{d.queue.length} in queue</span> : undefined}>
            {card ? (
              <div className="flashcard">
                <p className="question">{card.question}</p>
                {shown ? (
                  <>
                    <pre className="answer">{card.answer}</pre>
                    {card.explanation && <details className="explain"><summary>Explanation: how it works, an example, common mistakes</summary><Markdown text={card.explanation} />{card.explanation_id && <ReportProblem key={card.explanation_id} id={card.explanation_id} />}</details>}
                    <p className="muted small">How well did you recall it?</p>
                    <div className="grades">{GRADES.map((g) => <button key={g.rating} className={`grade grade-${g.rating}`} disabled={busy} onClick={() => void grade(g.rating)}><kbd>{g.key}</kbd> {g.label}</button>)}</div>
                  </>
                ) : (
                  <>
                    <p className="muted small">Answer from memory first, out loud or on paper.</p>
                    <button className="primary reveal" aria-label="Show answer (Space)" onClick={() => setShown(true)}>Show answer <kbd>Space</kbd></button>
                  </>
                )}
              </div>
            ) : <Empty>No cards due and no new cards left today. Come back tomorrow, or add cards with <code>careerbrew cards add</code>.</Empty>}
            {busy && <span className="muted small working" aria-live="polite">Saving…</span>}
            {note && !busy && !err && <p className="muted small" role="status">{note}</p>}
            {err && <p className="error" role="alert">{withCode(err)}</p>}
          </Section>
          <Section title="Progress" block="progress">
            <div className="stats">
              <Stat label="Due now" value={d.stats.due_now} />
              <Stat label="New" value={d.stats.new} sub={`of ${d.stats.total} cards`} />
              <Stat label="Reviewed today" value={d.stats.reviewed_today} />
              <Stat label="Retention" value={d.stats.retention_30d === null ? "—" : pct(d.stats.retention_30d)} sub={d.stats.retention_30d === null ? "no data yet" : "last 30 days"} />
            </div>
            <p className="muted small">Streak {d.stats.streak_days} d, {d.stats.new_per_day} new a day, leeches {d.stats.suspended.length}</p>
            {/* Two lines per topic: the narrow column stacked a five-row table block per topic (blind judge, 2026-10-22). */}
            <ul className="rows topics" aria-label="Cards by topic">{Object.entries(d.stats.by_topic).sort((a, b) => b[1].total - a[1].total).slice(0, 15).map(([t, r]) => (
              <li key={t}>
                <div className="row-head"><span>{t}</span><span className="nowrap"><strong>{r.due}</strong> due · {r.retention === null ? "no retention yet" : `${pct(r.retention)} kept`}</span></div>
                <div className="muted">{r.total} cards · {r.new} new</div>
              </li>
            ))}</ul>
            <details><summary>How this works</summary>
              <ul className="list small">
                <li>Recall before you reveal: answering from memory strengthens it more than re-reading.</li>
                <li>Spaced repetition (FSRS) schedules each card just before you would forget it.</li>
                <li>Topics are mixed, so you practise choosing the right approach.</li>
                <li>A daily cap on new cards keeps the review load steady.</li>
                <li>A card you fail 8 times is suspended as a leech: rewrite it smaller.</li>
              </ul>
            </details>
          </Section>
        </div>
      )}
    </State>
  );
}

// ---------- library ----------

const SECTION_LABELS: Record<string, string> = { career: "Career", "how-to": "How to", experience: "Experience", employer: "Current employer", plan: "Job search plan", article: "Saved articles", "past-job": "Past job", story: "Interview stories", job: "Job notes", "my-explanations": "My personal questions, explained (private)" };

// Topics that are acronyms keep their capitals (US-75 visual review: "Interview knowledge · Ai").
const ACRONYMS: Record<string, string> = { ai: "AI", ml: "ML", llm: "LLM", sql: "SQL", api: "API" };

// Public study cards come in sections "knowledge:<topic>".
function sectionLabel(section: string): string {
  if (section.startsWith("knowledge:")) {
    const topic = section.slice("knowledge:".length);
    return `Interview knowledge · ${ACRONYMS[topic] ?? `${topic.charAt(0).toUpperCase()}${topic.slice(1)}`}`;
  }
  return SECTION_LABELS[section] ?? section;
}

// A Library section with more pages than this starts folded.
const LIBRARY_OPEN_MAX = 10;

export function LibraryView({ pageId }: { pageId?: string }) {
  const q = useData(api.library);
  const page = useData(() => (pageId ? api.libraryPage(pageId) : Promise.resolve(null)), [pageId], { key: pageId ? `/api/library/${encodeURIComponent(pageId)}` : null });
  const [filter, setFilter] = useState("");
  return (
    <div className="split split-reader">
      <State q={q} label="pages">
        {(d) => {
          const shown = d.pages.filter((p) => p.title.toLowerCase().includes(filter.toLowerCase()));
          const groups = Object.entries(shown.reduce<Record<string, typeof shown>>((acc, p) => ({ ...acc, [p.section]: [...(acc[p.section] ?? []), p] }), {}));
          return (
            <Section title={`Pages (${d.pages.length})`} className="pane" block="pages">
              {d.pages.length === 0 ? <Empty>Nothing imported yet. Ask your agent to run the careerbrew-notion-import skill.</Empty> : (
                <>
                  <input aria-label="Filter pages" placeholder="Filter by title" value={filter} onChange={(e) => setFilter(e.target.value)} />
                  {/* A long section folds to its heading and count (about 850 pages in real use); it opens when you
                      filter, when it holds the open page, or when it is short. */}
                  {groups.map(([section, pages]) => (
                    <details key={section} className="tree" open={!!filter || pages.length <= LIBRARY_OPEN_MAX || pages.some((p) => p.id === pageId)}>
                      <summary><h3>{sectionLabel(section)} <span className="count">{pages.length}</span></h3></summary>
                      <ul>{pages.map((p) => (
                        <li key={p.id}>
                          <a href={`#/library/${encodeURIComponent(p.id)}`} aria-current={p.id === pageId ? "page" : undefined}>{p.title}</a>
                          {p.status ? <span className="tag">{p.status}</span> : null}
                          {p.truncated ? <span className="muted small"> (cut short in Notion)</span> : null}
                        </li>
                      ))}</ul>
                    </details>
                  ))}
                  {groups.length === 0 && <Empty>No page title matches “{filter}”.</Empty>}
                </>
              )}
            </Section>
          );
        }}
      </State>
      {pageId ? (
        <State q={page} label="the page">
          {(p) => p && (
            <article className="card reader" aria-label={p.title} data-block="reader">
              <About block="reader" />
              <p className="muted small">{sectionLabel(p.section)}{p.updated ? ` · updated ${friendlyDay(p.updated.slice(0, 10))}` : ""}</p>
              <h2>{p.title}</h2>
              <Markdown text={p.markdown || "(empty page)"} />
              {(p.section.startsWith("knowledge:") || p.section === "my-explanations") && <ReportProblem key={p.id} id={p.id} />}
            </article>
          )}
        </State>
      ) : (
        <div className="card reader reader-empty" data-block="reader"><About block="reader" /><Empty>Pick a page to read it here.</Empty></div>
      )}
    </div>
  );
}
