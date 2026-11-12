import { useEffect, useRef, useState } from "react";
import { About } from "./about";
import { api, hhmm } from "./api";
import type { NotificationAction, NotificationItem, Notifications, NotificationStatus } from "./api";
import { useAct } from "./act";
import { ActStatus } from "./act-status";
import { PAGE_SIZE } from "./order";
import { Fold, Paged } from "./pager";
import { useData } from "./useData";
import { keepDates } from "./text";
import { Empty, Section, State } from "./views";

// Important new events (issue #74). Each notification says what happened, why it matters, and the
// next step, with the same actions as a clickable pop-up: Open, Accept or Dismiss (suggestions), and
// Snooze. A pop-up click lands on #/notifications/<id>, which marks that one and scrolls to it.

const CLOSED: Record<string, string> = { accepted: "accepted", dismissed: "dismissed", closed: "no longer asks you", decided: "decided in the inbox" };

function state(n: NotificationItem): string {
  if (n.status === "snoozed") return `snoozed until ${hhmm(n.snooze_until)}`;
  if (n.status === "done" || n.status === "dismissed") return CLOSED[n.resolved] ?? n.status;
  return n.status === "new" ? `new · ${n.popup}` : "read";
}

function Item({ n, focused, onDone }: { n: NotificationItem; focused: boolean; onDone: (v: Notifications) => void }) {
  const a = useAct();
  const ref = useRef<HTMLLIElement>(null);
  useEffect(() => {
    if (focused) ref.current?.scrollIntoView?.({ block: "center" });
  }, [focused]);
  const act = async (action: NotificationAction) => {
    const v = await a.run(() => api.notificationAct(n.id, action));
    if (v) onDone(v);
  };
  const markRead = () => {
    if (n.status === "new") void act("read");
  };
  const external = n.url && !n.url.startsWith("#");
  return (
    <li ref={ref} data-notification={n.id} data-kind={n.kind} aria-current={focused ? "true" : undefined} className={`notification${focused ? " focused" : ""}`}>
      <div className="notification-head">
        <span className="tag" title={n.priority_why}>P{n.priority}</span>
        <strong>{keepDates(n.headline)}</strong>
        {n.count > 1 && <span className="tag">{n.count} times</span>}
        <span className="muted small">{state(n)}</span>
      </div>
      {n.title && <div className="notification-title">{keepDates(n.title)}</div>}
      <dl className="explain small">
        <dt>Why</dt><dd>{keepDates(n.why)}</dd>
        <dt>Next</dt><dd>{n.next}</dd>
        <dt>Priority</dt><dd>{n.priority} of 5: {n.priority_why}</dd>
      </dl>
      <div className="quick-add">
        {n.url && (external
          ? <a className="button" href={n.url} target="_blank" rel="noreferrer noopener" onClick={markRead}>{n.link_label}</a>
          : <a className="button" href={n.url} onClick={markRead}>{n.link_label}</a>)}
        {n.actions.includes("accept") && <button type="button" disabled={a.busy} onClick={() => act("accept")}>{n.accept_label}</button>}
        {n.actions.includes("dismiss") && <button type="button" className="secondary" disabled={a.busy} onClick={() => act("dismiss")}>Dismiss</button>}
        {n.actions.includes("snooze") && <button type="button" className="secondary" disabled={a.busy} onClick={() => act("snooze")}>Snooze</button>}
        {!n.url && n.status === "new" && <button type="button" className="secondary" disabled={a.busy} onClick={markRead}>Mark read</button>}
      </div>
      <ActStatus act={a} />
    </li>
  );
}

function Rules({ s, onChange }: { s: NotificationStatus; onChange: () => void }) {
  const a = useAct();
  if (!s.enabled) {
    const turnOn = async () => {
      if (await a.run(() => api.setConfig("notify_events", "true"))) onChange();
    };
    return (
      <div className="callout callout-warn" role="status" data-block="rules">
        Notifications are off. Turn them on to hear about pages, direct messages, review requests, and suggestions that matter.{" "}
        <button type="button" disabled={a.busy} onClick={turnOn}>Turn on</button>
        <ActStatus act={a} label="Saving…" />
        <About block="rules" />
      </div>
    );
  }
  return (
    <div data-block="rules">
      <About block="rules" />
      <p className="muted small">
        On: priority {s.min_priority} or more; quiet hours {s.quiet_hours || "none"}{s.quiet_now ? " (now)" : ""}; at most {s.daily_cap} pop-ups a day ({s.sent_today} today); pop-ups: {s.backend_note}. Change the rules in Setup → Settings (<code>notify_*</code>).
      </p>
    </div>
  );
}

export function NotificationsView({ focus }: { focus?: string }) {
  const q = useData(api.notifications, [], { live: true });
  const [override, setOverride] = useState<Notifications | null>(null);
  useEffect(() => setOverride(null), [q.data]);
  return (
    <State q={{ ...q, data: override ?? q.data }} label="notifications">
      {(d) => {
        const open = d.items.filter((n) => ["new", "snoozed", "read"].includes(n.status));
        const closed = d.items.filter((n) => !["new", "snoozed", "read"].includes(n.status));
        const focused = (n: NotificationItem) => n.id === focus;
        // Issue #146: open ones first, 10 at a time (unread, then by priority: the server's order); closed ones folded
        // with their count. While notifications are off, Turn on is the one action, so the rules come first.
        const rules = <Rules s={d.status} onChange={q.reload} />;
        return (
          <div className="stack">
            {!d.status.enabled && rules}
            <Section title={`Open (${open.length})`} block="open" action={<span className="muted small">{d.status.unread} unread</span>}>
              {open.length
                ? <Paged id="open" rows={open} size={PAGE_SIZE.open} noun="open notifications" focus={focused}>{(rows) => <ul className="rows">{rows.map((n) => <Item key={n.id} n={n} focused={focused(n)} onDone={setOverride} />)}</ul>}</Paged>
                : <Empty>Nothing needs you right now. New pages, direct messages, review requests, and suggestions show here.</Empty>}
            </Section>
            {closed.length > 0 && (
              <Fold title="Closed" count={closed.length} open={closed.some(focused)} block="closed">
                <Paged id="closed" rows={closed} size={PAGE_SIZE.closed} noun="closed notifications" focus={focused}>{(rows) => <ul className="rows">{rows.map((n) => <Item key={n.id} n={n} focused={focused(n)} onDone={setOverride} />)}</ul>}</Paged>
              </Fold>
            )}
            {d.status.enabled && rules}
          </div>
        );
      }}
    </State>
  );
}
