import { useEffect, useRef } from "react";
import { api } from "./api";

// Time saved (US-86): your own time from a result's open to Copy, Send, or Accept, for `careerbrew metrics
// time-saved`. The page sends only the action id, the step, and the seconds (`POST /api/timing`); never text.
// The seconds count only while the tab is visible, so a tab left in the background does not count as your time.

export type Saves = "pm_update" | "standup" | "inbox";
export type DoneStep = "copy" | "send" | "accept";

interface Open { start: number; hidden: number; hiddenAt: number | null; users: number; sent: boolean }

// Lifecycle: module scope of this browser page. An entry starts when the first block that shows the action's result
// shows it (useOpened) and is deleted when the last such block unmounts, so a new visit to the view is a new open.
// PM update and standup send their time once per open; the inbox (`each`) sends one per Accept, from the open or
// the previous Accept.
const opens = new Map<Saves, Open>();
const EACH: ReadonlySet<Saves> = new Set(["inbox"]);

function visibleMs(o: Open, now: number): number {
  return now - o.start - o.hidden - (o.hiddenAt === null ? 0 : now - o.hiddenAt);
}

let listening = false;
function listen(): void {
  if (listening) return;
  listening = true;
  document.addEventListener("visibilitychange", () => {
    const now = Date.now();
    for (const o of opens.values()) {
      if (document.hidden && o.hiddenAt === null) o.hiddenAt = now;
      else if (!document.hidden && o.hiddenAt !== null) {
        o.hidden += now - o.hiddenAt;
        o.hiddenAt = null;
      }
    }
  });
}

function send(saves: Saves, step: "open" | DoneStep, seconds: number): void {
  // A timing is a measurement, never the owner's action: a failed send changes nothing on the page.
  api.timing(saves, step, Math.round(Math.max(0, seconds) * 10) / 10).catch(() => undefined);
}

// Marks the result open once `ready` (its data shows); the open's own seconds are the wait from mount to data.
export function useOpened(saves: Saves, ready: boolean): void {
  const mounted = useRef(Date.now());
  useEffect(() => {
    if (!ready) return;
    listen();
    const now = Date.now();
    const o = opens.get(saves);
    if (o) o.users += 1;
    else {
      opens.set(saves, { start: now, hidden: 0, hiddenAt: document.hidden ? now : null, users: 1, sent: false });
      send(saves, "open", (now - mounted.current) / 1000);
    }
    return () => {
      const left = opens.get(saves);
      if (left && --left.users <= 0) opens.delete(saves);
    };
  }, [saves, ready]);
}

// Copy, Send, or Accept was pressed: sends the visible seconds since the open (once per open, or per Accept for the inbox).
export function timeDone(saves: Saves, step: DoneStep): void {
  const o = opens.get(saves);
  if (!o || (o.sent && !EACH.has(saves))) return;
  const now = Date.now();
  send(saves, step, visibleMs(o, now) / 1000);
  if (EACH.has(saves)) Object.assign(o, { start: now, hidden: 0, hiddenAt: o.hiddenAt === null ? null : now });
  else o.sent = true;
}
