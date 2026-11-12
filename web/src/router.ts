import { useCallback, useEffect, useMemo, useState } from "react";

// `group` builds the sidebar sections: Plan (your work and goals), Job search, System (careerbrew itself) (#66).
// `hint` is the one-line subtitle under each page title.
// `personalOnly` views read job-hunt data, which a corporate-role machine refuses with 403 (server.JOB_PATHS).
export const VIEWS = [
  { id: "dashboard", label: "Dashboard", group: "Plan", hint: "The most efficient work action now, today, this week, month, quarter, year, and at this company, with why." },
  { id: "teach", label: "Teach me", group: "Plan", hint: "One step at a time toward your upcoming goal." },
  { id: "today", label: "Today", group: "Plan", hint: "What to do next, and when." },
  { id: "standup", label: "Standup", group: "Plan", hint: "What you did on each item you report on since the last standup: a talking point, its status, and the evidence." },
  { id: "notifications", label: "Notifications", group: "Plan", hint: "Important new events: what happened, why it matters, and the next step. Act on each one here." },
  { id: "goals", label: "Goals", group: "Plan", hint: "Your long-term goals and the upcoming one. Only you set them; the schedule follows." },
  { id: "week", label: "Week", group: "Plan", hint: "Meetings and planned work for the days ahead." },
  { id: "jobs", label: "Job hunt", group: "Job search", personalOnly: true, hint: "Where you are in the season, and which roles are worth a loop." },
  { id: "interviews", label: "Interviews", group: "Job search", personalOnly: true, hint: "How each company interviews, with sourced tips and AI rules." },
  { id: "practice", label: "Practice", group: "Job search", personalOnly: true, hint: "Recall first, then grade. Spaced repetition picks the cards." },
  { id: "library", label: "Library", group: "Job search", personalOnly: true, hint: "Your notes imported from Notion." },
  { id: "work", label: "Work", group: "Plan", hint: "Your promotion case: coverage, gaps, and the evidence behind it." },
  { id: "review", label: "Review", group: "System", hint: "Is careerbrew working? Every outcome metric against its bar." },
  { id: "reports", label: "Reports", group: "System", hint: "One page per long run: what it did, what blocks you, and what to do next." },
  { id: "actions", label: "Actions", group: "System", hint: "Run any careerbrew command from the browser: the same actions the CLI and agents run." },
  { id: "setup", label: "Setup", group: "System", hint: "This machine, its plugins, connectors, and the day watcher." },
] as const;

export type ViewId = (typeof VIEWS)[number]["id"];

interface NavView { group: string; personalOnly?: boolean }
// The machine role from /api/overview: `undefined` while it loads, `null` when it could not load.
export type Role = string | null | undefined;

export const personalOnly = (v: NavView): boolean => !!v.personalOnly;
export const isBlocked = (v: NavView, role: Role): boolean => role === "corporate" && personalOnly(v);

// Sidebar groups with their visible views, in VIEWS order. A blocked view is left out, so a group whose every
// view is blocked disappears (issue #65). Until the role is known nothing shows: no blocked link flashes and
// no group pops in later and shifts the ones below it.
export function navGroups<V extends NavView>(views: readonly V[], role: Role): { group: string; views: V[] }[] {
  if (role === undefined) return [];
  const groups = new Map<string, V[]>();
  for (const v of views) if (!isBlocked(v, role)) groups.set(v.group, [...(groups.get(v.group) ?? []), v]);
  return [...groups].map(([group, shown]) => ({ group, views: shown }));
}

export interface Route { view: ViewId; param?: string }

// A hash is `#/<view>[/<param>][?<state>]`. The route is the part before "?"; the state after it is the app's own.
function routePart(hash: string): string {
  const q = hash.indexOf("?");
  return q < 0 ? hash : hash.slice(0, q);
}

export function parseHash(hash: string): Route {
  const [, view = "dashboard", param] = routePart(hash).replace(/^#\/?/, "/").split("/");
  const known = VIEWS.some((v) => v.id === view);
  return { view: (known ? view : "dashboard") as ViewId, param: param ? decodeURIComponent(param) : undefined };
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parseHash(window.location.hash));
  useEffect(() => {
    // Keep the same object while the view and its parameter stay: a change of a list's page is no new route.
    const on = () => setRoute((old) => {
      const next = parseHash(window.location.hash);
      return next.view === old.view && next.param === old.param ? old : next;
    });
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return route;
}

// The view's state in the hash after "?" (issue #146): each list's page and filter, Week's day count. The app owns
// this part and keeps bare tokens only: each key and value is 1 to 40 of a-z, 0-9, and "-". Anything else is
// dropped, so typed text never lands in the URL (and the browser's history), and no key can read as `token=`.
export type HashState = Readonly<Record<string, string>>;
const BARE = /^[a-z0-9][a-z0-9-]{0,39}$/;
export const isBare = (v: string): boolean => BARE.test(v);
const bareKey = (k: string) => BARE.test(k) && !k.endsWith("token");

export function parseState(hash: string): HashState {
  const q = hash.indexOf("?");
  const out: Record<string, string> = {};
  if (q < 0) return out;
  for (const pair of hash.slice(q + 1).split("&")) {
    const [k, v, ...rest] = pair.split("=");
    if (!rest.length && v !== undefined && bareKey(k) && BARE.test(v)) out[k] = v;
  }
  return out;
}

// The hash with `changes` applied: a null (or a value that is not a bare token) removes the key. Keys sort, so one
// state always reads as one URL.
export function withState(hash: string, changes: Record<string, string | null>): string {
  const state: Record<string, string> = { ...parseState(hash) };
  for (const [k, v] of Object.entries(changes)) {
    if (v !== null && bareKey(k) && BARE.test(v)) state[k] = v;
    else delete state[k];
  }
  const route = routePart(hash) || "#/";
  const pairs = Object.keys(state).sort().map((k) => `${k}=${state[k]}`);
  return pairs.length ? `${route}?${pairs.join("&")}` : route;
}

// Sent after the app changed its state in the hash. `history.replaceState` fires no hashchange, and paging should
// add no Back step, so the hooks listen for this too.
const STATE_EVENT = "careerbrew:hash-state";

// The view's state in the hash and a setter. Lifecycle: the state lives in the URL of this browser tab; a reload
// keeps it, and a link to another view (`#/<view>`) drops it.
export function useHashState(): [HashState, (changes: Record<string, string | null>) => void] {
  const [hash, setHash] = useState(() => window.location.hash);
  useEffect(() => {
    const on = () => setHash(window.location.hash);
    window.addEventListener("hashchange", on);
    window.addEventListener(STATE_EVENT, on);
    return () => {
      window.removeEventListener("hashchange", on);
      window.removeEventListener(STATE_EVENT, on);
    };
  }, []);
  const state = useMemo(() => parseState(hash), [hash]);
  const set = useCallback((changes: Record<string, string | null>) => {
    const next = withState(window.location.hash, changes);
    if (next === window.location.hash) return;
    window.history.replaceState(window.history.state, "", next);
    window.dispatchEvent(new Event(STATE_EVENT));
  }, []);
  return [state, set];
}
