import { useEffect, useState } from "react";
import { api, hhmm, localStamp, pmCopyPayload, webUrl } from "./api";
import type { DayLogView, InboxGroup, InboxItem, InboxView, PmCutCandidate, RowDetail } from "./api";
import { About } from "./about";
import { useAct } from "./act";
import { ActStatus } from "./act-status";
import { TaskLinks } from "./links";
import { PAGE_SIZE } from "./order";
import { Paged } from "./pager";
import type { Choice, Sort } from "./pager";
import { slug } from "./paging";
import { useData } from "./useData";
import type { Loadable } from "./useData";
import { friendlyDay, longDay, shortDay } from "./dates";
import { keepDates } from "./text";
import { CopyResult } from "./copy";
import { useCopy } from "./useCopy";
import { timeDone, useOpened } from "./timing";
import { CachedNote, DataTable, Empty, Section, State } from "./views";

// The daily log: done today, plan for tomorrow, the PM draft (copy or send), and what careerbrew needs from you.

function mailto(to: string, subject: string, body: string): string {
  return `mailto:${encodeURIComponent(to)}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
}

// The PM box's current text per day while Today shows it. Lifecycle: module scope of this browser page; PmReport
// sets the entry on every keystroke and deletes it when the box unmounts, so the map is empty on every other view.
// The top Copy update button reads it, so a copy right after an edit copies the edit, before the save on blur lands.
const pmBoxText = new Map<string, string>();

const dayLogFresh = (q: Loadable<DayLogView>) => q.data !== null && !q.loading && !q.error && q.cached === null;

// The day's heading with a one-click Copy update (issue #145), at the top of Today and Dashboard.
export function PmCopyBar() {
  const q = useData(api.daylog, [], { live: true });
  const { copied, copy, clear } = useCopy();
  const d = q.data;
  useOpened("pm_update", d !== null);
  return (
    <div className="day-head" data-block="copy-update" data-daylog-fresh={dayLogFresh(q)}>
      <h2>{d ? longDay(d.log.day) : "Today"}</h2>
      <button type="button" className="primary" disabled={!d} onClick={() => { if (!d) return; timeDone("pm_update", "copy"); const payload = pmCopyPayload(d.pm, pmBoxText.get(d.log.day) ?? d.pm.body); copy(payload.text, payload.html); }}>Copy update</button>
      {d ? (
        <span className="muted small" data-pm-source={d.pm.edited ? "edit" : "draft"}>
          {d.pm.edited ? "your edit" : "generated draft"}{d.pm.updated_at ? ` · updated ${hhmm(d.pm.updated_at)}` : ""}
        </span>
      ) : <State q={q} label="the PM update" compact>{() => null}</State>}
      <CopyResult copied={copied} via={d?.pm.via ?? "slack"} onClose={clear} />
      <About block="copy-update" />
    </div>
  );
}

function pmCuts(pm: DayLogView["pm"], body: string) {
  const context = pm.cut_context;
  if (context?.schema !== 1) {
    return { cuts: body === pm.body ? (pm.left_out ?? []).map((label) => ({ label, date: null })) : [], unknown: body !== pm.body };
  }
  if (context.candidates.length === 0) return { cuts: [], unknown: false };

  const spellings = new Map(context.candidates.flatMap(({ aliases }) => aliases.map(({ spelling, norm }) => [spelling, norm] as const)));
  const present = new Set<string>();
  let uncertain = false;
  let fence = "";
  for (const raw of body.split(/\r\n?|\n/)) {
    const content = raw.trim().replace(/^[-•◦ ]+/, "").trim();
    if (!content) continue;
    if (fence) {
      if (content[0] === fence[0] && content.length >= fence.length && [...content].every((char) => char === fence[0])) fence = "";
      continue;
    }
    const marker = content.match(/^(`{3,}|~{3,})/)?.[0];
    if (marker) {
      fence = marker;
      uncertain = true;
      continue;
    }
    const colon = content.indexOf(":");
    const prefix = colon < 0 ? "" : content.slice(0, colon);
    const label = prefix.replace(/[*_`]+/g, "").trim();
    const known = spellings.get(label);
    if (!label || /^(?:[#>]|\d+[.)]\s)/.test(label) || /[[\]<>|]/.test(prefix)) {
      uncertain = true;
    } else if (known !== undefined) {
      present.add(known);
    } else if (/^[ -~]+$/.test(label)) {
      present.add(label.toLowerCase());
    } else {
      uncertain = true;
    }
  }
  const absent = context.candidates.filter(({ aliases }) => !aliases.some(({ norm }) => present.has(norm)));
  return { cuts: uncertain ? [] : absent.map((candidate) => ({ label: candidate.label, date: cutDate(candidate) })), unknown: uncertain && absent.length > 0 };
}

function cutDate({ date_basis, recorded_at, report_day }: PmCutCandidate): string {
  if (date_basis === "saved" && recorded_at && /(?:Z|[+-]\d{2}:\d{2})$/i.test(recorded_at) && !Number.isNaN(Date.parse(recorded_at))) {
    const stamp = localStamp(recorded_at);
    return `Cut recorded ${friendlyDay(stamp.slice(0, 10))} ${hhmm(stamp)}`;
  }
  if (date_basis === "report-day" && report_day) {
    const day = friendlyDay(report_day);
    if (day !== report_day) return `Cut in update for ${day} (record time unavailable)`;
  }
  return "Date unavailable";
}

function PmReport({ pm, day }: { pm: DayLogView["pm"]; day: string }) {
  const [body, setBody] = useState(pm.body);
  const [saved, setSaved] = useState<string | null>(pm.edited ? "Your edits are saved for this day." : null);
  // Edits are kept per day on the server (issue #52), so a reload keeps them; an empty box resets to the draft.
  const act = useAct();
  const sendAct = useAct();
  const save = async (text: string) => {
    if (text === pm.body && !(pm.edited && text === "")) return;
    const r = await act.run(() => api.savePmDraft(day, text));
    if (r) setSaved(r.edited ? "Your edits are saved for this day." : "Back to the generated draft.");
  };
  const reset = async () => {
    setBody(pm.draft ?? pm.body);
    await save("");
  };
  const { copied, copy, clear } = useCopy();
  const [sent, setSent] = useState<string | null>(null);
  useOpened("pm_update", true);
  // A new answer (the fresh one after the page's cached one, issue #142, or a live refresh) brings the server's text.
  // The box takes it, unless it holds typing the server does not have yet: what the owner types is never dropped.
  const [base, setBase] = useState(pm.body);
  if (pm.body !== base) {
    setBase(pm.body);
    setSent(null);
    if (body === base) {
      setBody(pm.body);
      setSaved(pm.edited ? "Your edits are saved for this day." : null);
    }
  }
  useEffect(() => {
    pmBoxText.set(day, body);
    return () => { pmBoxText.delete(day); };
  }, [day, body]);
  const slackReady = Boolean(pm.to) && pm.slack_configured === true;
  const sendSlack = async () => {
    if (!slackReady) return;
    timeDone("pm_update", "send");
    const r = await sendAct.run(() => api.sendPm(`*${pm.subject}*\n${body}`));
    if (r) setSent(`Sent to ${r.sent.to}`);
  };
  const { cuts, unknown } = pmCuts(pm, body);
  return (
    <Section title="Update for your PM" action={<span className="muted small">a draft: edit it, then copy or send</span>}>
      {/* under the card's title, not above the card: there it stacked under Copy update's own line */}
      <About block="pm-update" />
      <label className="sr-only" htmlFor="pm-body">PM update</label>
      <textarea id="pm-body" aria-label="PM update" className="pm-body" rows={6} value={body} onChange={(e) => { setBody(e.target.value); clear(); }} onBlur={() => save(body)} />
      <div className="quick-add">
        <button type="button" onClick={() => { timeDone("pm_update", "copy"); const payload = pmCopyPayload(pm, body); copy(payload.text, payload.html); }}>{copied?.ok ? "Copied" : "Copy"}</button>
        {pm.via === "slack" ? (
          <button type="button" onClick={sendSlack} disabled={sendAct.busy || !slackReady || sent !== null}>{sent ?? (pm.to ? `Send to Slack ${pm.to}` : "Send to Slack")}</button>
        ) : (
          <a className="button" href={mailto(pm.to, pm.subject, body)} onClick={() => timeDone("pm_update", "send")}>{pm.to ? `Email ${pm.to}` : "Email…"}</a>
        )}
        {pm.edited && pm.draft !== undefined && <button type="button" disabled={act.busy} onClick={reset}>Reset to draft</button>}
        {saved && !act.busy && <span className="muted small" role="status">{saved}</span>}
        <ActStatus act={{ busy: act.busy, error: null }} label="Saving…" />
        <ActStatus act={{ busy: sendAct.busy, error: null }} label="Sending…" />
        {pm.via === "slack" && pm.to && pm.slack_configured === false && <span className="muted small">Connect Slack to send: <code>careerbrew connect slack</code>.</span>}
        {!pm.to && <span className="muted small">{pm.via === "slack" ? <>Set where it goes: <code>careerbrew config set pm_slack #channel-or-member-id</code></> : <>Set your PM's address: <code>careerbrew config set pm_email you@example.com</code></>}, or in Setup (pm_send_via switches Slack or email).</span>}
      </div>
      {unknown && <p className="muted small pm-cuts">Cannot verify which previously cut labels are absent from this text.</p>}
      {cuts.length > 0 && (
        <div className="muted small pm-cuts" data-pm-left-out>
          <p>Left out because you cut it before:</p>
          <ul>{cuts.map(({ label, date }) => <li key={label}>{label}{date && <> · <span>{date}</span></>}</li>)}</ul>
          <p>Bring one back: <code>careerbrew signals reset cut {cuts[0].label}</code></p>
        </div>
      )}
      {pm.why && pm.why.length > 0 && (
        <div className="pm-why" data-pm-why>
          <p className="muted small">Why it matters, per line (for you only: never copied or sent)</p>
          <ul className="small">{pm.why.map((w) => <li key={w.label}><strong>{w.label}</strong>: {w.note}</li>)}</ul>
        </div>
      )}
      {pm.checks && pm.checks.length > 0 && (
        <div className="pm-why" data-pm-checks>
          <p className="muted small">Content checks (for you only: fix the line, or keep it as it is)</p>
          <ul className="small">{pm.checks.map((c) => <li key={c.rule + c.line}><strong>{c.fix}</strong>: {c.line}</li>)}</ul>
        </div>
      )}
      <CopyResult copied={copied} via={pm.via} onClose={clear} />
      <ActStatus act={{ busy: false, error: act.error ?? sendAct.error }} />
    </Section>
  );
}

// Click a Plan or Done row to see where it came from and why it is there (issue #61).
function Row({ text, d }: { text: string; d?: RowDetail }) {
  if (!d) return <li>{text}</li>;
  return (
    <li>
      <details className="row-details">
        <summary>{text}</summary>
        <dl className="small">
          {d.url && <><dt>Source</dt><dd><a href={d.url} target="_blank" rel="noreferrer noopener">{d.url}</a></dd></>}
          {d.project && <><dt>Project</dt><dd>{d.project}</dd></>}
          <dt>Why</dt><dd>{keepDates(d.why)}</dd>
          {d.related.length > 0 && <><dt>Related</dt><dd>{d.related.join(", ")}</dd></>}
          {d.history.length > 0 && <><dt>History</dt><dd><ul className="rows">{d.history.map((h) => <li key={h.at + h.event}>{keepDates(localStamp(h.at))} {h.event}</li>)}</ul></dd></>}
          {d.task && <><dt>Task id</dt><dd><code>{d.task}</code></dd></>}
        </dl>
      </details>
    </li>
  );
}

// Issue #147: a suggestion's links (http and https only, in a new tab) and the view Accept lands it in.
function RowLinks({ links, lands }: { links?: InboxItem["links"]; lands?: InboxItem["lands"] }) {
  return (
    <>
      {links && links.length > 0 && <><dt>Links</dt><dd><TaskLinks links={links} /></dd></>}
      {lands && <><dt>Lands in</dt><dd><a href={lands.url}>{lands.label}</a></dd></>}
    </>
  );
}

// A signal's link (a PR, a review): only a full http(s) address becomes a link (webUrl), in a new tab.
function Out({ url, children }: { url: string; children: string }) {
  const href = webUrl(url);
  return href ? <a href={href} target="_blank" rel="noreferrer noopener">{children}</a> : <>{children}</>;
}

function Item({ item, onDone }: { item: InboxItem; onDone: (v: DayLogView) => void }) {
  const [text, setText] = useState("");
  const a = useAct();
  const act = async (fn: () => Promise<DayLogView>) => {
    const v = await a.run(fn);
    if (v) onDone(v);
  };
  const accept = item.kind === "goal" ? "Approve goal" : "Accept";
  return (
    <li data-inbox={item.kind}>
      {item.type === "proposal" && item.what ? (
        <>
          {item.maybe_done && <p className="callout small maybe-done" role="note">Maybe done already: <Out url={item.maybe_done.url}>{item.maybe_done.what}</Out>. Check, then Dismiss it if so.</p>}
          <dl className="explain">
            <dt>What</dt><dd>{keepDates(item.what)}</dd>
            <dt>Why this is suggested</dt><dd>{keepDates(item.why)}</dd>
            {item.added_by && <><dt>Source</dt><dd>{item.added_by}</dd></>}
            <RowLinks links={item.links} lands={item.lands} />
            <dt>{accept} will</dt><dd>{item.accept}</dd>
            <dt>Dismiss will</dt><dd>{item.dismiss}</dd>
          </dl>
        </>
      ) : <div>{item.text}</div>}
      {item.type === "proposal" ? (
        <div className="quick-add">
          <button type="button" disabled={a.busy} onClick={() => { timeDone("inbox", "accept"); act(() => api.inbox(item.id, "accept")); }}>{accept}</button>
          <button type="button" className="secondary" disabled={a.busy} onClick={() => act(() => api.inbox(item.id, "dismiss"))}>Dismiss</button>
        </div>
      ) : (
        <form className="quick-add" onSubmit={(e) => { e.preventDefault(); act(() => api.inbox(item.id, "answer", text)); }}>
          <input aria-label={`Answer: ${item.text}`} value={text} onChange={(e) => setText(e.target.value)} placeholder="Your answer" />
          <button type="submit" disabled={a.busy || !text.trim()}>Answer</button>
          <button type="button" className="secondary" disabled={a.busy} onClick={() => act(() => api.inbox(item.id, "dismiss"))}>Skip</button>
        </form>
      )}
      <ActStatus act={a} />
    </li>
  );
}

// Issue #147: alike suggestions (the children of one tracker, one agent's tasks for one reason) as one row. Accept all
// and Dismiss all are hard to undo, so each asks first; Review each shows every row with its own buttons.
function GroupRow({ g, members, onDone }: { g: InboxGroup; members: InboxItem[]; onDone: (v: DayLogView) => void }) {
  const [open, setOpen] = useState(false);
  const [ask, setAsk] = useState<"accept" | "dismiss" | null>(null);
  const a = useAct();
  const decide = async (action: "accept" | "dismiss") => {
    if (action === "accept") timeDone("inbox", "accept");
    const v = await a.run(() => api.inboxGroup(g.id, action, g.ids));  // the rows the confirm counted, not ones added since
    if (v) {
      setAsk(null);
      onDone(v);
    }
  };
  return (
    <li data-inbox-group={g.kind} data-group={g.id}>
      <dl className="explain">
        <dt>What</dt><dd><strong>{g.label}</strong></dd>
        <dt>Why this is suggested</dt><dd>{keepDates(g.why)}</dd>
        <dt>Source</dt><dd>{g.added_by}</dd>
        <RowLinks links={g.links} lands={g.lands} />
        <dt>Accept all will</dt><dd>{g.accept}</dd>
        <dt>Dismiss all will</dt><dd>{g.dismiss}</dd>
      </dl>
      <div className="quick-add">
        <button type="button" disabled={a.busy} onClick={() => setAsk("accept")}>Accept all</button>
        <button type="button" className="secondary" aria-expanded={open} onClick={() => setOpen(!open)}>Review each</button>
        <button type="button" className="secondary" disabled={a.busy} onClick={() => setAsk("dismiss")}>Dismiss all</button>
      </div>
      {ask && (
        <div className="callout callout-warn confirm" role="group" aria-label={ask === "accept" ? "Confirm Accept all" : "Confirm Dismiss all"}>
          <p>{ask === "accept" ? `Accept all ${g.count}? ${g.accept}` : `Dismiss all ${g.count}? ${g.dismiss}`} This is hard to undo.</p>
          <div className="quick-add">
            <button type="button" className="primary" disabled={a.busy} onClick={() => decide(ask)}>{ask === "accept" ? "Yes, accept all" : "Yes, dismiss all"}</button>
            <button type="button" className="secondary" disabled={a.busy} onClick={() => setAsk(null)}>Cancel</button>
          </div>
        </div>
      )}
      <ActStatus act={a} label={ask === "dismiss" ? "Dismissing…" : "Accepting…"} />
      {open && (
        <Paged id={`review-${g.id}`} rows={members} size={PAGE_SIZE.suggestions} noun={`rows in ${g.label}`}>
          {(shown) => <ul className="rows group-rows">{shown.map((i) => <Item key={i.id} item={i} onDone={onDone} />)}</ul>}
        </Paged>
      )}
    </li>
  );
}

// A suggestion careerbrew hid because its source shows it done: kept, with the signal, and Bring back.
function Expired({ item, onDone }: { item: InboxItem; onDone: (v: DayLogView) => void }) {
  const a = useAct();
  const back = async () => {
    const v = await a.run(() => api.inbox(item.id, "reopen"));
    if (v) onDone(v);
  };
  return (
    <li data-expired={item.kind}>
      <div>{keepDates(item.what ?? item.text)}</div>
      <div className="muted small">Done elsewhere: <Out url={item.expired?.url ?? ""}>{item.expired?.what ?? ""}</Out></div>
      <div className="quick-add"><button type="button" className="secondary" disabled={a.busy} onClick={back}>Bring back</button></div>
      <ActStatus act={a} />
    </li>
  );
}

function DoneElsewhere({ rows, onDone }: { rows: InboxItem[]; onDone: (v: DayLogView) => void }) {
  if (!rows.length) return null;
  return (
    <details className="fold-sub" data-done-elsewhere="">
      <summary><h3>Done elsewhere ({rows.length})</h3></summary>
      <p className="muted small">careerbrew hid these because data it already reads shows them done: a PR merged, a review you gave, CI green. They are kept. Bring back shows one again, for good.</p>
      <Paged id="done-elsewhere" rows={rows} size={PAGE_SIZE["done-elsewhere"]} noun="rows done elsewhere">
        {(shown) => <ul className="rows">{shown.map((i) => <Expired key={i.id} item={i} onDone={onDone} />)}</ul>}
      </Paged>
    </details>
  );
}

// Issue #146: Today shows the day log's parts as blocks of their own, in the order the owner acts on them (the PM
// update near the top, the asks, then the log). They share the reads of `useDayLog` (useDayLog.ts).

// The PM update draft: second on Today, under Copy update.
export function PmUpdate({ log }: { log: Loadable<DayLogView> }) {
  return (
    <div className="stack" data-block="pm-update" data-daylog-fresh={dayLogFresh(log)}>
      <State q={log} label="the PM update">{(d) => <PmReport key={d.log.day} pm={d.pm} day={d.log.day} />}</State>
    </div>
  );
}

const INBOX = {
  question: { block: "questions", title: "Questions for you", noun: "questions", label: "your questions", empty: "No questions: careerbrew has the context it needs." },
  proposal: { block: "suggestions", title: "careerbrew suggests", noun: "suggestions", label: "careerbrew's suggestions", empty: "No suggestions right now." },
} as const;

// Issue #147: what the inbox is, who adds to it, and what happens on Accept and Dismiss.
export const EXPLAINER =
  "A suggestion is a change careerbrew proposes: a task to plan, a ticket to track in a project, a goal. careerbrew adds them from what it reads in your work tools; plugins and agents add their own. Nothing changes until you press Accept. Dismiss hides one for good. Each row says why it is here, who added it, and what each button will do. When a source shows one already done, careerbrew moves it to Done elsewhere.";

// One row of the suggestions list: a suggestion, or a group of alike ones at the place of its oldest.
interface Entry { key: string; kind: string; project: string; priority: number | null; at: string; item?: InboxItem; group?: InboxGroup; members?: InboxItem[] }

function entries(items: InboxItem[], groups: InboxGroup[]): Entry[] {
  const byId = new Map(groups.map((g) => [g.id, g]));
  const seen = new Set<string>();
  const out: Entry[] = [];
  for (const i of items) {
    const g = i.group ? byId.get(i.group) : undefined;
    if (!g) out.push({ key: i.id, kind: i.kind, project: i.project ?? "", priority: i.priority ?? null, at: i.created_at ?? "", item: i });
    else if (!seen.has(g.id)) {
      seen.add(g.id);
      out.push({ key: g.id, kind: g.kind, project: g.project, priority: g.priority, at: g.created_at, group: g, members: items.filter((x) => x.group === g.id) });
    }
  }
  return out;
}

const rank = (e: Entry) => e.priority ?? Number.MAX_SAFE_INTEGER;
const CHOICES: Choice<Entry>[] = [
  { key: "project", label: "Project", of: (e) => e.project || "none", name: (v) => (v === "none" ? "no project" : v) },
  { key: "priority", label: "Priority", of: (e) => (e.priority ? `p${e.priority}` : "none"), name: (v) => (v === "none" ? "no ranked project" : `rank ${v.slice(1)} project`) },
];
const SORTS: Sort<Entry>[] = [
  // The server's order (issue #185): what you accept first, then oldest; kinds you keep dismissing sink.
  { key: "best", label: "Best first", cmp: () => 0 },
  { key: "oldest", label: "Oldest first", cmp: (a, b) => a.at.localeCompare(b.at) },
  { key: "newest", label: "Newest first", cmp: (a, b) => b.at.localeCompare(a.at) },
  { key: "priority", label: "Priority (rank 1 first)", cmp: (a, b) => rank(a) - rank(b) || a.at.localeCompare(b.at) },
  { key: "project", label: "Project", cmp: (a, b) => (a.project || "~").localeCompare(b.project || "~") || a.at.localeCompare(b.at) },
  { key: "kind", label: "Kind", cmp: (a, b) => a.kind.localeCompare(b.kind) || a.at.localeCompare(b.at) },
];

// careerbrew's questions or its suggestions, one page at a time, best first (what you accept, then oldest). Suggestions: what the inbox is, groups
// of alike rows, filters by kind, project, and priority, a sort, and the ones done elsewhere (issue #147).
export function InboxBlock({ type, items, onDone }: { type: "question" | "proposal"; items: Loadable<InboxView>; onDone: (v: DayLogView) => void }) {
  const b = INBOX[type];
  const rows = items.data?.inbox.filter((i) => i.type === type) ?? [];
  useOpened("inbox", type === "proposal" && rows.length > 0);
  return (
    <Section title={items.data ? `${b.title} (${rows.length})` : b.title} block={b.block}>
      {type === "proposal" && <p className="explainer">{EXPLAINER}</p>}
      <State q={items} label={b.label}>
        {(d) => type === "question" ? (
          rows.length ? (
            <Paged id={b.block} rows={rows} size={PAGE_SIZE[b.block]} noun={b.noun}>
              {(shown) => <ul className="rows">{shown.map((i) => <Item key={i.id} item={i} onDone={onDone} />)}</ul>}
            </Paged>
          ) : <Empty>{b.empty}</Empty>
        ) : (
          <>
            {rows.length ? (
              <Paged id={b.block} rows={entries(rows, d.groups ?? [])} size={PAGE_SIZE[b.block]} noun={b.noun} facet={(e) => e.kind} choices={CHOICES} sorts={SORTS}>
                {(shown) => <ul className="rows">{shown.map((e) => (e.group ? <GroupRow key={e.key} g={e.group} members={e.members ?? []} onDone={onDone} /> : <Item key={e.key} item={e.item as InboxItem} onDone={onDone} />))}</ul>}
              </Paged>
            ) : <Empty>{b.empty}</Empty>}
            <DoneElsewhere rows={d.expired ?? []} onDone={onDone} />
          </>
        )}
      </State>
    </Section>
  );
}

// Blockers, Risks, or Housekeeping under the next workday's plan; Housekeeping (a list of references) starts folded.
function DayList({ id, title, rows, folded }: { id: "blockers" | "risks" | "housekeeping"; title: string; rows: string[]; folded?: boolean }) {
  if (!rows.length) return null;
  const head = `${title} (${rows.length})`;
  const list = <Paged id={id} rows={rows} size={PAGE_SIZE[id]} noun={title.toLowerCase()}>{(shown) => <ul className="rows">{shown.map((x) => <li key={x}>{keepDates(x)}</li>)}</ul>}</Paged>;
  if (folded) return <details className="fold-sub"><summary><h3>{head}</h3></summary>{list}</details>;
  return <><h3>{head}</h3>{list}</>;
}

// Done today and the plan for the next workday, with what blocks it.
export function DayLog({ log }: { log: Loadable<DayLogView> }) {
  return (
    <div className="stack" data-block="day-log">
      <About block="day-log" />
      <State q={log} label="the daily log">
        {(d) => {
          const groups = Object.entries(d.log.done).filter(([, rows]) => rows.length);
          return (
            <div className="split">
              <Section title={`Done today (${shortDay(d.log.day)})`}>
                {groups.length ? groups.map(([g, rows]) => (
                  <div key={g}>
                    <h3>{g} ({rows.length})</h3>
                    <Paged id={`done-${slug(g)}`} rows={rows.map((text, i) => ({ text, i }))} size={PAGE_SIZE.done} noun={`done rows in ${g}`}>
                      {(shown) => <ul className="rows">{shown.map((x) => <Row key={`${g}-${x.i}`} text={x.text} d={d.log.details?.done[g]?.[x.i]} />)}</ul>}
                    </Paged>
                  </div>
                )) : <Empty>Nothing recorded yet. The watcher adds your commits, PRs, tickets, and meetings as they happen.</Empty>}
              </Section>
              <Section title={`Plan for ${shortDay(d.log.tomorrow)}`}>
                {d.log.planned.length ? <ol className="list">{d.log.planned.map((x, i) => <Row key={x} text={x} d={d.log.details?.planned[i]} />)}</ol> : <Empty>{d.log.today_holds ? `Nothing left for ${shortDay(d.log.tomorrow)}: today's plan holds every open task.` : "No task fits yet: add one in Week."}</Empty>}
                <DayList id="blockers" title="Blockers" rows={d.log.blockers} />
                <DayList id="risks" title="Risks" rows={d.log.risks ?? []} />
                <DayList id="housekeeping" title="Housekeeping" rows={d.log.housekeeping ?? []} folded />
              </Section>
            </div>
          );
        }}
      </State>
    </div>
  );
}

// The kept logs of earlier days, last on Today.
export function EarlierDays({ log }: { log: Loadable<DayLogView> }) {
  const [day, setDay] = useState("");
  const [text, setText] = useState<string | null>(null);
  const act = useAct();
  const pick = async (d: string) => {
    setDay(d);
    setText(null);
    if (d) { const r = await act.run(() => api.daylogFor(d)); if (r) setText(r.markdown); }
  };
  const days = log.data?.history ?? [];
  if (days.length < 2) return null;
  return (
    <Section title="Earlier days" block="earlier-days">
      <label className="field">Day <select aria-label="Earlier day" value={day} onChange={(e) => pick(e.target.value)}>
        <option value="">Choose…</option>
        {days.slice(1).map((d) => <option key={d} value={d}>{d}</option>)}
      </select></label>
      <ActStatus act={act} label="Loading…" />
      {text && <pre className="page">{text}</pre>}
    </Section>
  );
}

// Settings: every default careerbrew uses, where its value comes from, and an inline edit (same rules as `careerbrew config set`).
// Settings by topic, in this order; a key with no matching prefix is General.
const SETTING_GROUPS: [string, RegExp][] = [
  ["Work day", /^(work_|timezone$|questions_per_day$|start_)/],
  ["PM update", /^(pm_|project_priority$|metrics_share$)/],
  ["Notifications", /^notify_/],
  ["Standup", /^standup_/],
  ["Work feed and projects", /^(feed_|project_)/],
  ["Web app", /^(serve_|web_)/],
  ["Data and backups", /^backup_/],
  ["Voice practice", /^stt_/],
];
function settingGroups<T extends { key: string }>(rows: readonly T[]): [string, T[]][] {
  const out = new Map<string, T[]>([...SETTING_GROUPS.map(([g]) => [g, [] as T[]] as [string, T[]]), ["General", []]]);
  for (const r of rows) out.get(SETTING_GROUPS.find(([, re]) => re.test(r.key))?.[0] ?? "General")!.push(r);
  return [...out].filter(([, items]) => items.length);
}

export function SettingsEditor() {
  const q = useData(api.config);
  const [rows, setRows] = useState<DayLogSettings | null>(null);
  const [edit, setEdit] = useState<{ key: string; value: string } | null>(null);
  const act = useAct();
  const data = rows ?? q.data?.settings ?? null;
  if (!data) return <Section title="Settings" block="settings"><State q={q} label="settings">{() => null}</State></Section>;
  const save = async () => {
    if (!edit) return;
    const r = await act.run(() => api.setConfig(edit.key, edit.value));
    if (r) { setRows(r.settings); setEdit(null); }
  };
  return (
    <Section title="Settings" block="settings" action={<span className="muted small">defaults unless you change them; agents use <code>careerbrew config set</code></span>}>
      {!rows && <CachedNote q={q} />}
      {/* About 50 settings: one fold per topic, with how many you changed, so the page is not 8,000 px of rows. */}
      {settingGroups(data).map(([group, items], i) => {
        const changed = items.filter((s) => s.source !== "default").length;
        return (
          <details key={group} className="settings-group" open={i === 0}>
            <summary><h3>{group} <span className="count">{items.length}</span>{changed > 0 && <span className="muted"> · {changed} changed</span>}</h3></summary>
      <DataTable>
        <thead><tr><th>Setting</th><th>Value</th><th>From</th><th></th></tr></thead>
        <tbody>{items.map((s) => (
          <tr key={s.key} className="setting-row">
            <td><code>{s.key}</code><div className="muted small">{s.help}</div></td>
            <td>{edit?.key === s.key
              ? <input aria-label={`New value for ${s.key}`} value={edit.value} onChange={(e) => setEdit({ key: s.key, value: e.target.value })} onKeyDown={(e) => e.key === "Enter" && save()} />
              : String(Array.isArray(s.value) ? s.value.join(",") : s.value)}</td>
            <td className="muted small">{s.source}</td>
            <td>{edit?.key === s.key
              ? <button type="button" disabled={act.busy} onClick={save}>Save</button>
              : <button type="button" className="secondary" onClick={() => setEdit({ key: s.key, value: String(Array.isArray(s.value) ? s.value.join(",") : s.value ?? "") })}>Change</button>}</td>
          </tr>
        ))}</tbody>
      </DataTable>
          </details>
        );
      })}
      <ActStatus act={act} label="Saving…" />
    </Section>
  );
}

type DayLogSettings = import("./api").Setting[];
