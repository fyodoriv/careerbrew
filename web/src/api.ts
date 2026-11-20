// Typed client for the local careerbrew API (`careerbrew serve`, 127.0.0.1 only).
// The token arrives once in the URL fragment (#token=...), moves to sessionStorage,
// and is sent as a header; the fragment never reaches the server. It lasts across
// restarts (issue #141), and the server keeps it in an HttpOnly cookie, so a reload,
// a new tab, or a bookmark gets it back from `GET /api/session` (signIn below).

import { slackCopy } from "./pm-copy";

export type Status = "KEEP" | "WATCH" | "PIVOT" | "no-data";

export interface Meeting { start: string; end: string; title: string }
// Issue #143: a task's links and its own why travel with every task row.
export interface TaskLink { label: string; url: string }
export interface TaskContext { links?: TaskLink[]; task_why?: string }
export interface ScheduleItem extends TaskContext { task: string; title: string; start: string; end: string; carry_over: boolean; reason: string; project?: string | null }
export interface Plan { day: string; meetings: Meeting[]; free_minutes: number; focus_minutes: number; calibration: number; schedule: ScheduleItem[]; unplaced: string[]; unplaced_titles?: string[]; calendar_errors: string[]; calendar_connected?: boolean }
export interface NextStep extends TaskContext { kind: string; text: string; plain?: string; task?: string; title?: string; reason?: string }
export interface ResultRow { id: string; at: string; kind: string; title: string; source: string; feed_source?: string }
export type SourceHealthStatus = "healthy" | "missing" | "stopped" | "stale" | "error" | "unknown";
export interface SourceHealth { name: string; required: boolean; health: SourceHealthStatus; acquired_at: string | null; checked_at: string | null; age_seconds: number | null; max_age_minutes: number }
export interface FeedHealth {
  version: 1; as_of: string; status: SourceHealthStatus;
  reason: "current" | "no-sources" | "direct-gap" | "adapters-stopped" | "required-gap" | "optional-gap" | "invalid-config";
  running: boolean; mode: "direct" | "adapter" | "none"; required: number; healthy: number;
  sources: SourceHealth[]; next_expiry: string | null; commands: string[];
  row_review: { status: "current" | "due" | "unknown"; checked_at: string | null; age_seconds: number | null; max_age_minutes: 60 };
}
export interface Today { plan: Plan; next: NextStep; results: ResultRow[]; summary: Record<string, number> }
export interface FeedAsk { id: string; source: string; kind: string; at: string; title: string; url: string; waiting_minutes: number; work_minutes: number; overdue: boolean }
// `limit`: the most asks one answer may hold (server.ASKS_LIMIT); past it, the rest are only in the terminal (issue #275).
export interface FeedAsks { asks: FeedAsk[]; total: number; limit: number; events: number; overdue_hours: number; closed_today?: number }
// The last day of a week plan says what was left: `unplaced` (no room in the days) and `too_big` (more than the focus time of any whole day).
export interface TooBig { task: string; title?: string; minutes: number }
export interface WeekDay { day: string; weekday: string; workday: boolean; meetings: Meeting[]; schedule: ScheduleItem[]; free_minutes: number; unplaced: string[]; unplaced_titles?: string[]; too_big?: TooBig[]; day_focus_minutes?: number; calendar_connected?: boolean }
export interface Week { days: WeekDay[] }
export interface Posting { id: string; company: string; title: string; location: string; region: string; want: number; rating: number | null; url: string; source: string; has_loop: boolean }
export interface TierRow { company: string; title: string; want: number; p_offer: number; ev: number; url: string; posting_id?: string; chosen?: boolean }
export interface Strategy { phase?: string; readiness?: number; steps?: string[]; tiers?: Record<"dream" | "target" | "practice", TierRow[]>; skill_gaps?: [string, number][]; used_this_season?: string[] }
export interface BoardJob { id: string; company: string; role: string; stage: string | null; want: number | null; salary: number | null; location: string[] | null; remote: string | null; url: string | null; reason_no: string | null; last_their_contact: string | null; created: string | null }
export interface Jobs { strategy: Strategy & { suggested?: Record<"dream" | "target" | "practice", TierRow[]> }; postings: Posting[]; rejected: Record<string, number>; board: BoardJob[] }
export interface Flashcard { id: string; question: string; answer: string; explanation?: string; explanation_id?: string; topic: string; tags: string[]; deck: string; mode: "review" | "new" }
export interface CardStats { total: number; new: number; due_now: number; due_today: number; reviewed_today: number; retention_30d: number | null; suspended: string[]; streak_days: number; new_per_day: number; by_topic: Record<string, { total: number; due: number; new: number; retention: number | null }> }
export interface Cards { queue: Flashcard[]; stats: CardStats }
export type Rating = "again" | "hard" | "good" | "easy";
export interface LibraryEntry { id: string; title: string; section: string; path?: string | null; updated?: string | null; status?: string | null; truncated?: boolean | null; file: string }
export interface Library { pages: LibraryEntry[] }
export interface LibraryPage extends LibraryEntry { markdown: string }
export interface CompanyRow { id: string; name: string; steps: string[]; ai: string; checked: string | null; prep_score: number | null }
export interface Patterns { companies: number; step_share: Record<string, number>; typical_order: string[]; transitions: [string, string, number][]; ai_policy: Record<string, number>; ai_shared_patterns: { text: string }[] }
export interface Interviews { companies: CompanyRow[]; patterns: Patterns }
export interface LoopStep { id: string; name?: string; description?: string; typical_minutes?: string; evaluated?: string[]; tips?: { text: string }[]; traps?: { text: string }[]; company_notes: string }
export interface InterviewDetail { company: string; known: boolean; loop: LoopStep[]; company_tips: { text: string }[]; values: string[]; ai_policy?: { allowed: string; summary: string; checked: string }; ai_shared?: { text: string }[]; stale?: { reason: string; at: string } | null; source_list: Record<string, { title: string; url: string; checked?: string }> }
export interface Competency { id: string; name: string; coverage: number; need: number; credit: number; priority: number; how_to_close: string }
export interface Task { id: string; title: string; impact: number; urgency: number; estimate_h: number; competencies: string[]; due?: string | null; score: number; promo_bonus: number; project?: string | null }
export interface ProjectRow { name: string; title: string; group: string; parent: string; links: string[]; children: number; depth: number; sources: number; stale: number; archived: boolean; rank?: number | null }
export interface WatchedSource { kind: string; ref: string; checked_at: string | null; stale: boolean; reference: boolean; reader: string; status?: string }
// A prep project's readiness gate (issue #276): tool, links, questions, quiz; the quiz result is counts only.
export interface PrepGate { gate: string; text: string; done: boolean; at: string | null; note: string; fix: string }
export interface Prep { event_on: string; done: number; total: number; ready: boolean; gates: PrepGate[]; quiz: { at: string; asked: number; missed: number; score: number } | null; quizzes: number }
export interface WatchedProject { name: string; title?: string; group: string; parent: string; archived: boolean; stale_after_days: number; stale: number; last_checked_at: string | null; latest_update: { at: string; kind: string; text?: string } | null; sources: WatchedSource[]; prep?: Prep }
// Active now (issue #198): one ranked project, the same numbers as `careerbrew work priority show`.
// Agent activity on Work (issue #272): each background agent's run, its state from its last event.
export type AgentStatus = "running" | "quiet" | "done" | "failed";
export interface AgentRun { agent: string; run: string; project: string; status: AgentStatus; title: string; url: string; at: string; started_at: string; events: number }
export interface Agents { counts: Record<AgentStatus, number>; quiet_minutes: number; groups: { project: string; running: number; last_at: string; runs: AgentRun[] }[] }
export interface PriorityRow { name: string; rank: number; match: string[]; open_tasks: number; today: number }
export interface Work { promo: { ladder?: string; level?: string; overall?: number; competencies: Competency[]; error?: string }; gaps: Competency[]; tasks: Task[]; projects?: ProjectRow[]; watched?: WatchedProject[]; evidence: { id: string; kind: string; title: string; at: string; tags: string[] }[] }
export interface FocusItem extends TaskContext { title: string; why: string; ref?: string | null; project?: string | null }
export interface Horizon { id: string; label: string; items: FocusItem[]; empty: string }
export interface StaleSource { project: string; kind: string; ref: string; age_days: number | null; limit: number }
export interface TomorrowItem extends TaskContext { title: string; project?: string | null; start: string; end: string; ref: string }
export interface Tomorrow { day: string; items: TomorrowItem[]; empty: string }
export interface Focus { at: string; ladder?: string; company_ladder?: boolean; stale?: StaleSource[]; horizons: Horizon[]; tomorrow?: Tomorrow }
// One Dashboard block (`/api/focus/<id>`, issue #142): a horizon, Tomorrow, or the stale sources; the ladder blocks name the ladder.
export interface FocusBlock { id: string; at: string; horizon?: Horizon; ladder?: string; company_ladder?: boolean; tomorrow?: Tomorrow; stale?: StaleSource[] }
// Issue #147: a proposal says where it came from (added_by), its http(s) links, the view Accept lands it in, its
// project and that project's rank in project_priority (priority), its group of alike rows, and "maybe done".
export interface Signal { what: string; url: string; at: string }
export interface InboxItem {
  id: string; type: "proposal" | "question"; kind: string; text: string; status: string; created_at?: string;
  what?: string; why?: string; accept?: string; dismiss?: string;
  added_by?: string; links?: TaskLink[]; lands?: { label: string; url: string } | null; project?: string; priority?: number | null;
  group?: string; maybe_done?: Signal; expired?: Signal;
}
export interface InboxGroup {
  id: string; kind: string; count: number; label: string; why: string; accept: string; dismiss: string; added_by: string;
  links: TaskLink[]; lands: { label: string; url: string } | null; project: string; priority: number | null; created_at: string; ids: string[];
}
export interface InboxView { inbox: InboxItem[]; groups?: InboxGroup[]; expired?: InboxItem[] }
// Issue #268: a promise you made in a sent message, tracked as a project until done (`work promise list`).
export interface PromiseRow { name: string; title: string; to: string; due: string; overdue: boolean; made_at: string; url: string; parent: string; next: "research" | "draft" | "status"; steps: Record<string, string>; closed: string }
export interface Promises { promises: PromiseRow[]; steps: Record<string, string> }
export interface ChecklistStep { id: string; title: string; why: string; view: string; command: string; done: boolean }
export interface Checklist { steps: ChecklistStep[]; done: number; total: number; hidden: boolean; show: boolean }
// today_holds: work tasks the rest of today's plan still holds, so they are not planned for tomorrow too.
export interface DayLog { day: string; tomorrow: string; done: Record<string, string[]>; planned: string[]; today_holds?: number; blockers: string[]; risks?: string[]; housekeeping?: string[]; questions: string[]; advice: string[]; details?: { done: Record<string, RowDetail[]>; planned: RowDetail[] } }
export interface RowDetail { title: string; task?: string; kind?: string; url: string; project: string; why: string; related: string[]; history: { at: string; event: string }[] }
// Issue #214: one owner-only note per PM update line (who benefits, what it should change, a checked result); never sent.
export interface PmWhy { label: string; note: string }
export interface PmCheck { rule: string; line: string; fix: string }  // issue #226: a content rule a line breaks, for the owner only
export interface PmCutCandidate {
  label: string;
  aliases: { spelling: string; norm: string }[];
  recorded_at: string | null;
  report_day: string | null;
  date_basis: "saved" | "report-day" | "unavailable";
}
export interface DayLogView extends InboxView { log: DayLog; markdown: string; pm: { subject: string; body: string; draft?: string; edited?: boolean; updated_at?: string; to: string; via: "slack" | "email"; slack_configured?: boolean; left_out?: string[]; cut_context?: { schema: 1; candidates: PmCutCandidate[] }; why?: PmWhy[]; checks?: PmCheck[] }; history: string[] }
export interface Setting { key: string; value: unknown; source: string; default: unknown; help: string }
// Issue #110: what you did on each row you report on since the last standup.
// Issue #229: `extra` marks an action an agent passed (Slack, meetings, mail); `agent_written` a talking point an agent wrote.
export interface StandupAction { title: string; at: string; when: string; kinds: string[]; nouns: string[]; repo: string; url: string; keys: string[]; via: string; extra?: boolean }
// Issue #271: `proofs` are the live checks a row passed (only with `--proof`); a row that failed never reaches the view.
export interface StandupProof { source: string; url: string; checked_at: string; quote: string; when: string }
export interface StandupRow { title: string; status: string; owner: string; mine: boolean; keys: string[]; url: string; talking_point: string; talking_points?: string[]; agent_written?: boolean; actions: StandupAction[]; proofs?: StandupProof[] }
export interface StandupGroup { label: string; never_rows: boolean; talking_point: string; talking_points?: string[]; actions: StandupAction[] }
export interface Standup {
  window: { since: string; until: string; tz: string; start: string; end: string; days: number[]; time: string; minutes: number; mode: "previous-day" | "since-last"; compares: string };
  rows: StandupRow[]; groups: StandupGroup[]; hidden: number; has_rows: boolean; cleared?: boolean; saved_at: string; updated?: string; page: string;
  counts: { actions: number; rows: number; shown: number; yours: number; matched: number; hidden: number; on_rows: number; by_group: [string, number][]; talked?: number; extra?: number; gated?: boolean; passed?: number; failed?: number };
}
// Time saved (US-86): minutes saved per local ISO week and action; `time` says which time a cell used (web: your time up
// to copy, send, or accept; cli: run time). `start` is the week's Monday.
export interface TimeSavedCell { runs: number; runs_web: number; runs_cli: number; opened: number; minutes_saved: number; minutes_spent: number; time: "web" | "cli" | "web+cli" | "none" }
export interface TimeSavedWeek { week: string; start: string; runs: number; minutes_saved: number; actions: Record<string, TimeSavedCell> }
export interface TimeSaved { computed: string; this_week: string; measured_baselines: number; baselines: Record<string, { minutes: number; kind: string }>; weeks: TimeSavedWeek[]; minutes_saved_total: number; runs_total: number }
export type NotificationAction = "read" | "accept" | "dismiss" | "snooze";
export interface NotificationItem { id: string; kind: string; source: string; headline: string; title: string; why: string; next: string; priority: number; priority_why: string; count: number; at: string; status: "new" | "snoozed" | "read" | "done" | "dismissed"; snooze_until: string; resolved: string; url: string; link_label: string; accept_label: string; actions: NotificationAction[]; popup: string }
export interface NotificationStatus { enabled: boolean; min_priority: number; quiet_hours: string; daily_cap: number; batch_minutes: number; snooze_minutes: number; backend_kind: string; backend_note: string; quiet_now: boolean; sent_today: number; open: number; unread: number }
export interface Notifications { items: NotificationItem[]; status: NotificationStatus }
export interface ActionArg { name: string; flag: string | null; kind: "flag" | "choice" | "int" | "number" | "text"; choices: string[] | null; required: boolean; many: boolean; help: string; default: unknown }
export interface Action { name: string; command: string; help: string; args: ActionArg[]; ui: boolean; terminal_only: string | null; confirm: string | null }
export interface ActionResult { action: string; argv: string[]; exit: number; output: string }
export interface Metric { metric: string; value: number | null; status: Status; success: number; pivot: number; question: string; action: string; higher_is_better: boolean }
export interface Review { metrics: Metric[] }
// A run page's content-free summary (`careerbrew report latest --json`); the page itself comes from /api/reports/<id>.
export interface RunItem { id: string; label: string; evidence?: string; why?: string; command: string }
export interface RunSummary { id: string; run: string; title: string; at: string; status: "ok" | "blocked"; personal: boolean; counts: Record<string, number>; commands: string[]; done: RunItem[]; blockers: RunItem[]; next: RunItem[] }
export interface ConnectorStatus { kind: string; origin: string; secret: string | null; secret_ok: boolean | null; missing: string[]; ready: boolean; steps: string[] }
export interface Setup { role: string; plugins: { name: string; version: string; extends: string | null; exec: boolean; provides: string[] }[]; policy: Record<string, unknown>; connectors: ConnectorStatus[] }
export interface Overview { role: string; plugins: { name: string }[]; watcher: { state: string; running: boolean; reloads: number; today: Record<string, number> } }

export interface Exercise { id: string; order: number; pattern: string; difficulty: string; title: string; statement: string; signature: string; lesson: string; starter: string; hints: string[]; tests: { args: unknown; expected: unknown; note: string }[]; worked_example?: { title: string; statement: string; solution: string }; watch_out?: string[] }
export interface Approval { id: string; title: string; detail: string; by: string; at: string; status: string; note: string }
export interface Mistake { at: string; source: string; pattern: string; kind: string; detail: string; tip: string; repeat: boolean }
export interface TeachSummary { steps: number; minutes: number; by_kind: Record<string, number>; mistake_repeat_rate: number | null }
export interface Step {
  kind: "approve" | "mistake" | "card" | "lesson" | "exercise" | "validate" | "resume" | "done" | "goal";
  title: string; why: string; started: string; setup?: string;
  approval?: Approval; mistake?: Mistake; card?: Flashcard; pattern?: string; lesson?: string; prompt?: string; example?: { title: string; statement: string; solution: string };
  exercise?: Exercise; attempts?: number; job?: TierRow & { posting_id: string; skills?: string[] }; checklist?: string[]; summary?: TeachSummary;
}
export interface Grade { passed: number; total: number; correct: boolean; complexity: { expected: string; ratio: number | null; ok: boolean } | null; style: string[]; score: number }
export interface SubmitResult { grade: Grade; error: string | null; failing: { i: number; args: unknown; expected: unknown; got?: string; error?: string; line?: number }[]; mistakes: Mistake[]; solution: string | null }
export interface TeachReply { message: string; next: Step; result?: SubmitResult }
export interface Teach { step: Step; summary: TeachSummary; rules_version: string }
export interface Goal { id?: string; kind: string; title: string; date?: string | null; company?: string | null; topics?: string[]; minutes_per_day?: number }
export interface Goals { queue: Goal[]; long_term: { id?: string; text: string; by?: string | null }[]; upcoming: Goal | null; history: Goal[]; focus: { why: string; activities: string[] }; days_left: number | null; kinds: string[]; plan?: Plan }
export interface Improvement { signal: string; ref: string; proposal: string; auto?: string }

const TOKEN_KEY = "careerbrew-token";

export function captureToken(hash: string = window.location.hash): string | null {
  const m = hash.match(/token=([A-Za-z0-9_-]+)/);
  if (m) {
    sessionStorage.setItem(TOKEN_KEY, m[1]);
    const rest = hash.replace(/[#&]?token=[A-Za-z0-9_-]+/, "");
    window.history.replaceState(null, "", window.location.pathname + (rest.startsWith("#") ? rest : rest ? `#${rest}` : "#/dashboard"));
  }
  return sessionStorage.getItem(TOKEN_KEY);
}

// Sent when the server answers 401 (a rotated or expired token, or a tab that never had one): the app signs in again or shows the sign-in page.
export const SIGNED_OUT_EVENT = "careerbrew:signed-out";
export type SignIn = "ok" | "signed-out" | "offline";

export function hasToken(): boolean {
  return !!sessionStorage.getItem(TOKEN_KEY);
}

// A tab that holds the token asks the server for the sign-in cookie, so a reload, a new tab, or a bookmark of this origin signs in too.
export async function keepSignIn(): Promise<void> {
  const token = sessionStorage.getItem(TOKEN_KEY);
  if (!token) return;
  const res = await fetch("/api/session", { method: "POST", headers: { "X-Careerbrew-Token": token, "Content-Type": "application/json" }, body: "{}" });
  if (res.status === 401) window.dispatchEvent(new Event(SIGNED_OUT_EVENT));
}

// Sign this tab in from the HttpOnly cookie (the server returns the token to this app's own pages only).
export async function signIn(): Promise<SignIn> {
  let res: Response;
  try {
    res = await fetch("/api/session");
  } catch {
    return "offline";  // the web app does not run (or restarts): `careerbrew open` starts it
  }
  const body = res.ok ? ((await res.json().catch(() => ({}))) as { token?: string }) : {};
  if (!body.token) {
    sessionStorage.removeItem(TOKEN_KEY);  // a rotated or expired token never comes back
    return "signed-out";
  }
  sessionStorage.setItem(TOKEN_KEY, body.token);
  return "ok";
}

// The sign-in page's button: the server opens a signed-in tab in the default browser, as `careerbrew open` does. Null when it did.
export async function openSignedInTab(view: string): Promise<string | null> {
  try {
    const res = await fetch("/api/session/open", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ view }) });
    if (res.ok) return null;
    const body = (await res.json().catch(() => ({}))) as { error?: string };
    return body.error ?? res.statusText;
  } catch {
    return "the web app does not answer; run `careerbrew open` in a terminal";
  }
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const token = sessionStorage.getItem(TOKEN_KEY) ?? "";
  const res = await fetch(path, { ...init, headers: { "X-Careerbrew-Token": token, "Content-Type": "application/json", ...(init?.headers ?? {}) } });
  const body = await res.json().catch(() => undefined);
  if (res.status === 401) window.dispatchEvent(new Event(SIGNED_OUT_EVENT));
  if (!res.ok) throw new ApiError(res.status, (body as { error?: string } | undefined)?.error ?? res.statusText);
  if (body === undefined) throw new ApiError(res.status, "the server sent a response that is not valid JSON");
  return body as T;
}

// A GET view whose path keys the page's cache of the last answer (issue #142).
function view<T>(path: string): (() => Promise<T>) & { key: string } {
  return Object.assign(() => call<T>(path), { key: path });
}

export interface DrillView { item: { id: string; topic: string; prompt: string; mode: string } | null; criteria: { id: string; text: string }[]; stt: boolean }
export interface DrillVoiceReply { transcript: string; source: "audio" | "typed"; row: { met: string[]; missed: string[]; score: number; rating: string }; feedback: string[] }

export const api = {
  overview: () => call<Overview>("/api/overview"),
  changes: () => call<{ version: string }>("/api/changes"),
  tabKey: () => call<{ key: string | null }>("/api/tab-key"),
  today: view<Today>("/api/today"),
  feedAsks: view<FeedAsks>("/api/feed/asks"),
  feedHealth: () => call<FeedHealth>("/api/feed/health"),
  // null: the server's default (the 50 newest); a number asks for that many (issue #275).
  feedAsksUpTo: (limit: number | null) => call<FeedAsks>(`/api/feed/asks${limit ? `?limit=${limit}` : ""}`),
  week: (days = 7) => call<Week>(`/api/week?days=${days}`),
  jobs: view<Jobs>("/api/jobs"),
  interviews: view<Interviews>("/api/interviews"),
  interview: (company: string) => call<InterviewDetail>(`/api/interviews/${encodeURIComponent(company)}`),
  work: view<Work>("/api/work"),
  priority: view<{ projects: PriorityRow[] }>("/api/priority"),
  agents: view<Agents>("/api/agents"),
  focus: view<Focus>("/api/focus"),
  focusBlock: (id: string) => call<FocusBlock>(`/api/focus/${encodeURIComponent(id)}`),
  daylog: view<DayLogView>("/api/daylog"),
  inboxItems: view<InboxView>("/api/inbox"),
  checklist: view<Checklist>("/api/checklist"),
  daylogFor: (day: string) => call<{ day: string; markdown: string }>(`/api/daylog/${encodeURIComponent(day)}`),
  standup: view<Standup>("/api/standup"),
  inbox: (id: string, action: "accept" | "dismiss" | "answer" | "reopen", text = "") => call<DayLogView>(`/api/inbox/${encodeURIComponent(id)}/${action}`, { method: "POST", body: JSON.stringify({ text }) }),
  // Accept all or Dismiss all of one group (issue #147): the page asks first, then sends yes and the ids it showed, so
  // rows that joined the group since stay open (`left`).
  inboxGroup: (id: string, action: "accept" | "dismiss", ids: string[]) => call<DayLogView & { result: { decided: number; failed: { id: string; error: string }[]; left: number } }>(`/api/inbox-group/${encodeURIComponent(id)}/${action}`, { method: "POST", body: JSON.stringify({ yes: true, ids }) }),
  timeSaved: view<TimeSaved>("/api/time-saved"),
  promises: view<Promises>("/api/promises"),
  closePromise: (name: string, how: "done" | "dropped") => call<Promises>(`/api/promises/${encodeURIComponent(name)}/${how}`, { method: "POST", body: "{}" }),
  timing: (saves: string, step: string, seconds: number) => call<{ logged: boolean }>("/api/timing", { method: "POST", body: JSON.stringify({ saves, step, seconds }) }),
  savePmDraft: (day: string, text: string) => call<{ saved: boolean; edited: boolean }>("/api/pm/draft", { method: "POST", body: JSON.stringify({ day, text }) }),
  sendPm: (text: string) => call<{ sent: { via: string; to: string; ts: string } }>("/api/pm/send", { method: "POST", body: JSON.stringify({ text }) }),
  actions: view<{ actions: Action[] }>("/api/actions"),
  runAction: (name: string, values: Record<string, unknown>, confirm = false) => call<ActionResult>(`/api/actions/${encodeURIComponent(name)}`, { method: "POST", body: JSON.stringify({ values, confirm }) }),
  notifications: view<Notifications>("/api/notifications"),
  notificationAct: (id: string, action: NotificationAction) => call<Notifications>(`/api/notifications/${encodeURIComponent(id)}/${action}`, { method: "POST", body: "{}" }),
  config: view<{ settings: Setting[] }>("/api/config"),
  setConfig: (key: string, value: string) => call<{ settings: Setting[] }>("/api/config", { method: "POST", body: JSON.stringify({ key, value }) }),
  review: view<Review>("/api/review"),
  reports: view<{ reports: RunSummary[] }>("/api/reports"),
  report: (id: string) => call<{ summary: RunSummary; html: string }>(`/api/reports/${encodeURIComponent(id)}`),
  setup: view<Setup>("/api/setup"),
  cards: view<Cards>("/api/cards"),
  reviewCard: (id: string, rating: Rating, seconds?: number) => call<{ review: { due: string; leech: boolean } }>(`/api/cards/${encodeURIComponent(id)}/review`, { method: "POST", body: JSON.stringify({ rating, seconds }) }),
  library: view<Library>("/api/library"),
  libraryPage: (id: string) => call<LibraryPage>(`/api/library/${encodeURIComponent(id)}`),
  addTask: (t: { title: string; hours: number; impact?: number; urgency?: number; competencies?: string[]; due?: string }) => call<{ task: Task }>("/api/tasks", { method: "POST", body: JSON.stringify(t) }),
  doneTask: (id: string, hours: number) => call<{ task: Task }>(`/api/tasks/${id}/done`, { method: "POST", body: JSON.stringify({ hours }) }),
  teach: view<Teach>("/api/teach"),
  drill: view<DrillView>("/api/drill"),
  drillVoice: (body: { item: string; text?: string; audio?: string; mime?: string }) => call<DrillVoiceReply>("/api/drill/voice", { method: "POST", body: JSON.stringify(body) }),
  teachComplete: (step: Step, result: Record<string, unknown>) => call<TeachReply>("/api/teach/complete", { method: "POST", body: JSON.stringify({ step, result }) }),
  teachDraft: (id: string, code: string) => call<{ saved: boolean }>("/api/teach/draft", { method: "POST", body: JSON.stringify({ id, code }) }),
  teachFeedback: (text: string, step: string) => call<unknown>("/api/teach/feedback", { method: "POST", body: JSON.stringify({ text, step }) }),
  improvements: view<{ improvements: Improvement[] }>("/api/teach/improvements"),
  goals: view<Goals>("/api/goals"),
  saveGoals: (body: { upcoming?: Goal | null; queue?: Goal[]; long_term?: Goals["long_term"] }) => call<Goals>("/api/goals", { method: "POST", body: JSON.stringify(body) }),
  chooseJob: (posting_id: string, tier: string) => call<{ chosen: Record<string, string> }>("/api/jobs/choose", { method: "POST", body: JSON.stringify({ posting_id, tier }) }),
  unchooseJob: (posting_id: string) => call<{ chosen: Record<string, string> }>("/api/jobs/unchoose", { method: "POST", body: JSON.stringify({ posting_id }) }),
  flagExplanation: (id: string, note: string) => call<{ flag: { note: string; at: string } }>(`/api/explanations/${encodeURIComponent(id)}/flag`, { method: "POST", body: JSON.stringify({ note }) }),
  watch: (action: "on" | "off" | "pause" | "resume" | "reload") => call<{ watcher: { state: string; running: boolean } }>(`/api/watch/${action}`, { method: "POST", body: "{}" }),
};

// The address to link to when `raw` is a full http(s) URL with a host; null for javascript:, data:, file:, or a bare path.
export function webUrl(raw: string): string | null {
  try {
    const u = new URL(raw.trim());
    return (u.protocol === "http:" || u.protocol === "https:") && u.hostname ? u.href : null;
  } catch {
    return null;
  }
}

const MD_LINK = /\[([^\]\n]*)\]\((https?:\/\/[^\s)]+)\)/g;
const AUTOLINK = /<(https?:\/\/[^\s>|]+)>/g;

// Slack gets the draft with rich links and a plain fallback. Email keeps its plain subject and body.
export function pmCopyPayload(pm: Pick<DayLogView["pm"], "subject" | "via">, body: string): { text: string; html?: string } {
  if (pm.via === "slack") return slackCopy(body);
  const text = body
    .replace(MD_LINK, (_m: string, label: string, url: string) => (label.trim() && label.trim() !== url ? `${label} (${url})` : url))
    .replace(AUTOLINK, "$1")
    .replace(/\s+$/, "");
  return { text: `${pm.subject}\n\n${text}` };
}

export function pmCopyText(pm: Pick<DayLogView["pm"], "subject" | "via">, body: string): string {
  return pmCopyPayload(pm, body).text;
}

export function hhmm(iso: string): string {
  return iso.length >= 16 ? iso.slice(11, 16) : iso;
}

// "YYYY-MM-DD HH:MM" in this browser's time zone (US-75). A stamp with an offset ("Z", "+00:00") is converted, so an
// approval or a task added in UTC shows the local time; a stamp without one is careerbrew's local time, shown as written.
export function localStamp(iso: string): string {
  const asWritten = iso.slice(0, 16).replace("T", " ");
  if (!/(?:[zZ]|[+-]\d{2}:?\d{2})$/.test(iso)) return asWritten;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return asWritten;
  const two = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}`;
}

export function localDate(iso: string): string {
  return localStamp(iso).slice(0, 10);
}

export function pct(x: number | null | undefined): string {
  return x === null || x === undefined ? "—" : `${Math.round(x * 100)}%`;
}

export function duration(minutes: number): string {
  const m = Math.max(0, Math.round(minutes));
  if (m < 60) return `${m}m`;
  return m % 60 ? `${Math.floor(m / 60)}h ${m % 60}m` : `${m / 60}h`;
}
