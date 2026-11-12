import { useEffect, useRef, useState } from "react";
import { About } from "./about";
import { api, localStamp } from "./api";
import type { Goal, Goals, Rating, Step, SubmitResult, Teach } from "./api";
import { useAct } from "./act";
import { ActStatus } from "./act-status";
import { useData } from "./useData";
import { friendlyDay } from "./dates";
import { keepDates } from "./text";
import { DataTable, Empty, Markdown, ReportProblem, Section, Stat, State } from "./views";

// ---------- teacher mode: one step at a time ----------

function Elapsed({ since }: { since: string }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, []);
  const s = Math.max(0, Math.round((now - Date.parse(since)) / 1000));
  return <span className="count" aria-label="Time on this step">{Math.floor(s / 60)}:{String(s % 60).padStart(2, "0")}</span>;
}

function CodeEditor({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  return (
    <textarea
      ref={ref} className="code-editor" aria-label="Your solution" spellCheck={false} value={value}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key !== "Tab") return;
        e.preventDefault(); // Tab indents, like an editor
        const el = e.currentTarget;
        const { selectionStart: a, selectionEnd: b } = el;
        onChange(value.slice(0, a) + "    " + value.slice(b));
        requestAnimationFrame(() => { if (ref.current) ref.current.selectionStart = ref.current.selectionEnd = a + 4; });
      }}
    />
  );
}

// Code typed for an exercise survives failed attempts, reloads, and restarts: it is saved to the
// encrypted data folder (teach/drafts.json) through the API, never in browser storage.
const drafts = new Map<string, string>();
const saveTimers = new Map<string, ReturnType<typeof setTimeout>>();
function saveDraft(id: string, code: string) {
  clearTimeout(saveTimers.get(id));
  saveTimers.set(id, setTimeout(() => { api.teachDraft(id, code).catch(() => undefined); }, 500));
}

function ExerciseStep({ step, onDone }: { step: Step; onDone: (r: Record<string, unknown>) => Promise<void> }) {
  const ex = step.exercise!;
  const [code, setCodeState] = useState(() => drafts.get(ex.id) ?? ex.starter);
  const setCode = (v: string) => { drafts.set(ex.id, v); setCodeState(v); saveDraft(ex.id, v); };
  const [busy, setBusy] = useState(false);
  return (
    <div className="stack">
      <Markdown text={ex.statement} />
      <pre className="code">{ex.signature}</pre>
      {ex.tests.length > 0 && (
        <DataTable><thead><tr><th>Example input</th><th>Expected</th><th>Note</th></tr></thead>
          <tbody>{ex.tests.map((t, i) => <tr key={i}><td><code>{JSON.stringify(t.args)}</code></td><td><code>{JSON.stringify(t.expected)}</code></td><td className="muted small">{t.note}</td></tr>)}</tbody></DataTable>
      )}
      {ex.watch_out && ex.watch_out.length > 0 && (
        <div className="callout"><strong>Watch out</strong> (mistakes you have made before):<ul>{ex.watch_out.map((w, i) => <li key={i}>{w}</li>)}</ul></div>
      )}
      {ex.worked_example && (
        <details className="callout" open><summary><strong>Worked example: {ex.worked_example.title}</strong> (same pattern, already solved)</summary>
          <Markdown text={ex.worked_example.statement} /><pre className="code">{ex.worked_example.solution}</pre>
          <p className="muted small">Read it line by line: what does each line do, and why? Then use the same idea here.</p>
        </details>
      )}
      {ex.hints.length > 0 && <div className="callout"><strong>Hints unlocked</strong><ol className="list small">{ex.hints.map((h) => <li key={h}>{h}</li>)}</ol></div>}
      <CodeEditor value={code} onChange={setCode} />
      <div className="buttons">
        <button className="primary" disabled={busy} onClick={() => { setBusy(true); void onDone({ code }).finally(() => setBusy(false)); }}>{busy ? "Running…" : "Run and submit"}</button>
        <span className="muted small">Runs locally with time and memory limits. Graded on correctness, then complexity, then readability.</span>
      </div>
    </div>
  );
}

function Result({ r }: { r: SubmitResult }) {
  const g = r.grade;
  return (
    <div className="stack">
      <div className="stats">
        <Stat label="Tests" value={`${g.passed}/${g.total}`} />
        <Stat label="Score" value={`${Math.round(g.score * 100)}%`} />
        <Stat label="Complexity" value={g.complexity ? (g.complexity.ok ? "ok" : "too slow") : "—"} sub={g.complexity?.expected} />
        <Stat label="Style" value={g.style.length ? g.style.length : "clean"} sub={g.style.join(", ")} />
      </div>
      {r.error && <p className="callout callout-error">{r.error}</p>}
      {r.failing.length > 0 && (
        <DataTable><thead><tr><th>Input</th><th>Expected</th><th>Got</th></tr></thead>
          <tbody>{r.failing.map((f) => <tr key={f.i}><td><code>{JSON.stringify(f.args)}</code></td><td><code>{JSON.stringify(f.expected)}</code></td><td><code>{f.error ? `${f.error}${f.line ? ` (line ${f.line})` : ""}` : f.got}</code></td></tr>)}</tbody></DataTable>
      )}
      {r.solution && <details><summary>Reference solution</summary><pre className="code">{r.solution}</pre></details>}
    </div>
  );
}

function CardStep({ step, onDone }: { step: Step; onDone: (r: Record<string, unknown>) => Promise<void> }) {
  const [shown, setShown] = useState(false);
  const c = step.card!;
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (!shown && e.key === " ") { e.preventDefault(); setShown(true); return; }
      const r = ({ "1": "again", "2": "hard", "3": "good", "4": "easy" } as Record<string, Rating>)[e.key];
      if (shown && r) void onDone({ rating: r });
    };
    window.addEventListener("keydown", on);
    return () => window.removeEventListener("keydown", on);
  });
  return (
    <div className="flashcard">
      <p className="question">{c.question}</p>
      {shown ? (
        <>
          <pre className="answer">{c.answer}</pre>
          {c.explanation && <details className="explain"><summary>Explanation</summary><Markdown text={c.explanation} />{c.explanation_id && <ReportProblem id={c.explanation_id} />}</details>}
          <div className="grades">{(["again", "hard", "good", "easy"] as Rating[]).map((r, i) => <button key={r} className={`grade grade-${r}`} onClick={() => void onDone({ rating: r })}><kbd>{i + 1}</kbd> {r[0].toUpperCase() + r.slice(1)}</button>)}</div>
        </>
      ) : <button className="primary reveal" aria-label="Show answer (Space)" onClick={() => setShown(true)}>Show answer <kbd>Space</kbd></button>}
    </div>
  );
}

function StepBody({ step, onDone }: { step: Step; onDone: (r: Record<string, unknown>) => Promise<void> }) {
  const [text, setText] = useState("");
  switch (step.kind) {
    case "goal":
      return <p className="callout">Set your upcoming goal in <a href="#/goals">Goals</a>, or run <code>{step.setup}</code>. Lessons start once a goal is set.</p>;
    case "lesson":
      return (
        <div className="stack">
          <Markdown text={step.lesson ?? ""} />
          {step.example && (
            <details className="callout" open><summary><strong>Example: {step.example.title}</strong></summary>
              <Markdown text={step.example.statement} /><pre className="code">{step.example.solution}</pre></details>
          )}
          <label className="field-block">{step.prompt}<textarea aria-label="Explain it back" value={text} onChange={(e) => setText(e.target.value)} placeholder="Explain it in your own words" /></label>
          <div className="buttons"><button className="primary" disabled={!text.trim()} onClick={() => void onDone({ confirmed: true, explanation: text })}>I understand it</button><button onClick={() => void onDone({ confirmed: false })}>Not yet</button></div>
        </div>
      );
    case "exercise":
      return <ExerciseStep key={step.exercise!.id} step={step} onDone={onDone} />;
    case "card":
      return <CardStep key={step.card!.id} step={step} onDone={onDone} />;
    case "validate":
      return (
        <div className="stack">
          <p><strong>Your answer:</strong> {step.card!.answer}</p>
          {step.card!.explanation ? <details open><summary>Explanation to compare with</summary><Markdown text={step.card!.explanation} /></details> : <p className="muted small">No public explanation yet; check it against what you know.</p>}
          <textarea aria-label="Corrected answer" value={text} onChange={(e) => setText(e.target.value)} placeholder="If your answer is wrong or unclear, write the fix here" />
          <div className="buttons"><button className="primary" onClick={() => void onDone({ decision: "keep" })}>It is right: keep</button><button disabled={!text.trim()} onClick={() => void onDone({ decision: "fix", answer: text })}>Save my fix</button><button onClick={() => void onDone({ decision: "drop" })}>Drop it</button></div>
        </div>
      );
    case "approve":
      return (
        <div className="stack">
          <p className="muted small">Requested by {step.approval!.by} · {keepDates(localStamp(step.approval!.at))}</p>
          {step.approval!.detail && <Markdown text={step.approval!.detail} />}
          <input aria-label="Note for the agent" value={text} onChange={(e) => setText(e.target.value)} placeholder="Optional note for the agent" />
          <div className="buttons"><button className="primary" onClick={() => void onDone({ decision: "approved", note: text })}>Approve</button><button onClick={() => void onDone({ decision: "rejected", note: text })}>Reject</button></div>
        </div>
      );
    case "resume":
      return (
        <div className="stack">
          <p><strong>{step.job!.title}</strong> at {step.job!.company}{step.job!.url && <> · <a href={step.job!.url} target="_blank" rel="noreferrer noopener">open the posting</a></>}</p>
          <ol className="list">{(step.checklist ?? []).map((c) => <li key={c}>{c}</li>)}</ol>
          <div className="buttons"><button className="primary" onClick={() => void onDone({ sent: true })}>I sent it</button><button onClick={() => void onDone({ sent: false })}>Skip for now</button></div>
        </div>
      );
    case "mistake":
      return (
        <div className="stack">
          <p className="callout callout-warn">{step.mistake!.detail}</p>
          <p><strong>How to prevent it:</strong> {step.mistake!.tip}</p>
          <div className="buttons"><button className="primary" onClick={() => void onDone({})}>Got it</button></div>
        </div>
      );
    default:
      return step.summary ? <p className="muted">Today: {step.summary.steps} step(s), {step.summary.minutes} min.</p> : null;
  }
}

// What a step is, without the time the server read it: the cached and the fresh answer of one step share it.
function stepId(s: Step): string {
  return `${s.kind}:${s.card?.id ?? s.exercise?.id ?? s.approval?.id ?? s.mistake?.at ?? s.job?.posting_id ?? s.pattern ?? s.title}`;
}

export function TeachView() {
  const q = useData(api.teach);
  const [step, setStep] = useState<Step | null>(null);
  const [message, setMessage] = useState("");
  const [result, setResult] = useState<SubmitResult | null>(null);
  const [fb, setFb] = useState("");
  const [fbSent, setFbSent] = useState(false);
  const current = step ?? q.data?.step ?? null;
  // The step this view loaded shows from the page's cache first (issue #142), and the server stamps `started` on
  // every read, so it is keyed on what it is: the fresh answer keeps what the owner typed. A step the teacher hands
  // out after an answer is always new.
  const stepKey = step ? `next:${step.started}` : current ? `loaded:${stepId(current)}` : "";
  const act = useAct();
  const fbAct = useAct();
  const done = async (r: Record<string, unknown>) => {
    if (!current) return;
    const reply = await act.run(() => api.teachComplete(current, r));
    if (!reply) return;
    setMessage(reply.message);
    setResult(reply.result ?? null);
    setStep(reply.next);
    q.reload(); // refresh today's steps and minutes
  };
  return (
    <State q={q} label="your next step">
      {(d: Teach) => current && (
        <div className="split split-main">
          <div className="stack" data-block="step">
            <About block="step" />
            {message && <p className="callout teacher-says" role="status"><strong>Teacher:</strong> {message}</p>}
            {result && <Section title="Your last attempt"><Result r={result} /></Section>}
            {/* a card shows its question once, in the card itself; the block is named for what you do */}
            <Section title={current.kind === "card" ? "Recall this card" : current.title} className="study" action={current.kind !== "done" ? <Elapsed key={current.started} since={current.started} /> : undefined}>
              <p className="muted small step-why"><span className="tag">{current.kind}</span> {current.why}</p>
              <fieldset className="plain" disabled={act.busy}><StepBody key={stepKey} step={current} onDone={done} /></fieldset>
              <ActStatus act={act} label="Checking…" />
            </Section>
          </div>
          <div className="stack">
            <GoalCard />
            <Section title="Today" block="today-steps">
              <div className="stats">
                <Stat label="Steps" value={d.summary.steps} />
                <Stat label="Minutes" value={d.summary.minutes} />
              </div>
              <p className="muted small">Mistakes that repeated: {d.summary.mistake_repeat_rate === null ? "none yet" : `${Math.round(d.summary.mistake_repeat_rate * 100)}%`}</p>
            </Section>
            <Section title="Help the teacher improve" block="feedback">
              {fbSent ? <p className="muted small" role="status">Thanks. It becomes a fix in the next teacher update; most fixes come from what the teacher notices on its own.</p> : (
                <form className="stack" onSubmit={(e) => { e.preventDefault(); void fbAct.run(() => api.teachFeedback(fb, current.kind)).then((r) => r !== undefined && setFbSent(true)); }}>
                  <textarea aria-label="Feedback for the teacher" value={fb} onChange={(e) => setFb(e.target.value)} placeholder="What should the teacher do differently?" />
                  <button type="submit" disabled={fbAct.busy || !fb.trim()}>Send feedback</button>
                  <ActStatus act={fbAct} label="Sending…" />
                </form>
              )}
              <Improvements />
            </Section>
          </div>
        </div>
      )}
    </State>
  );
}

function Improvements() {
  const q = useData(api.improvements);
  if (!q.data?.improvements.length) return null;
  return (
    <details><summary>What the teacher noticed ({q.data.improvements.length})</summary>
      <ul className="list small">{q.data.improvements.slice(0, 10).map((i, n) => <li key={n}><span className="tag">{i.signal}</span> {i.proposal}{i.auto && <span className="muted"> ({i.auto})</span>}</li>)}</ul>
    </details>
  );
}

function GoalCard() {
  const q = useData(api.goals);
  return (
    <Section title="Upcoming goal" block="goal" action={<a className="small" href="#/goals">Change</a>}>
      <State q={q} label="your upcoming goal">
        {(d) => d.upcoming ? (
          <>
            <p className="next-how"><strong>{d.upcoming.title}</strong></p>
            <p className="muted small">{d.days_left !== null ? `${d.days_left} ${d.days_left === 1 ? "day" : "days"} left · ` : ""}{d.upcoming.minutes_per_day} min a day · focus: {d.focus.why}</p>
          </>
        ) : <Empty>No upcoming goal yet. <a href="#/goals">Set one</a>; the teacher and your schedule follow it.</Empty>}
      </State>
    </Section>
  );
}

// ---------- goals: only the owner sets them ----------

export function GoalsView() {
  const q = useData(api.goals);
  // The page's cache shows the last answer first (issue #142). The fresh answer reaches the same form, which keeps
  // what the owner typed and takes the fresh lists: a save never drops a goal added elsewhere in the meantime.
  return <State q={q} label="your goals">{(d: Goals) => <GoalsForm data={d} reload={q.reload} />}</State>;
}

function GoalsForm({ data, reload }: { data: Goals; reload: () => void }) {
  const blank: Goal = { kind: "interview", title: "", date: "", company: "", topics: [], minutes_per_day: 90 };
  const [queue, setQueue] = useState<Goal[]>(data.queue);
  const [editing, setEditing] = useState<number | null>(data.queue.length ? null : -1); // -1 = new goal
  const [g, setG] = useState<Goal>(blank);
  const [topics, setTopics] = useState("");
  const [lt, setLt] = useState(data.long_term);
  const [newLt, setNewLt] = useState("");
  const [msg, setMsg] = useState("");
  const [plan, setPlan] = useState(data.plan ?? null);
  const [currentId, setCurrentId] = useState(data.upcoming?.id ?? null);
  // A new answer (the fresh one after the cached one, or a reload) replaces the lists the server owns. The draft
  // stays; an edit stays on its goal, found by id, and becomes a new goal if that goal is gone.
  const [shown, setShown] = useState(data);
  if (data !== shown) {
    setShown(data);
    setQueue(data.queue);
    setLt(data.long_term);
    setCurrentId(data.upcoming?.id ?? null);
    if (editing !== null && editing >= 0) setEditing(data.queue.findIndex((x) => x.id === queue[editing]?.id));
  }
  const open = (i: number) => {
    const src = i >= 0 ? queue[i] : blank;
    setG({ ...blank, ...src });
    setTopics((src.topics ?? []).join(", "));
    setEditing(i);
  };
  const act = useAct();
  const ltAct = useAct();
  const saveQueue = async (next: Goal[]) => {
    setMsg("");
    const r = await act.run(() => api.saveGoals({ queue: next }));
    if (!r) return;
    setQueue(r.queue);
    setCurrentId(r.upcoming?.id ?? null);
    setPlan(r.plan ?? null);
    setEditing(null);
    setMsg("Saved. Today's schedule and the teacher now follow " + (r.upcoming ? `"${r.upcoming.title}"` : "your plan") + ".");
  };
  const submit = () => {
    const goal = { ...g, topics: topics.split(",").map((t) => t.trim()).filter(Boolean), date: g.date || null, company: g.company || null };
    void saveQueue(editing !== null && editing >= 0 ? queue.map((x, i) => (i === editing ? goal : x)) : [...queue, goal]);
  };
  const saveLt = async (items: Goals["long_term"]) => {
    if (!(await ltAct.run(() => api.saveGoals({ long_term: items })))) return false;
    setLt(items);
    reload();
    return true;
  };
  return (
    <div className="split">
      <Section title="Upcoming goals" block="upcoming" action={<button type="button" onClick={() => open(-1)}>Add a goal</button>}>
        <p className="muted small">The nearest goal still ahead drives today's schedule and the teacher; when its date passes, the next one takes over.</p>
        <ul className="rows">{queue.map((x, i) => (
          <li key={x.id ?? i}>
            <div className="row-head">
              <span><strong>{x.title}</strong> {x.id === currentId && <span className="badge badge-keep">drives today</span>}<span className="muted small meta-line">{x.kind}{x.date ? <>, <span className="nowrap">{friendlyDay(x.date)}</span></> : ""}, <span className="nowrap">{x.minutes_per_day} min/day</span></span></span>
              <span className="buttons"><button type="button" aria-label={`Edit ${x.title}`} onClick={() => open(i)}>Edit</button><button type="button" aria-label={`Remove ${x.title}`} disabled={act.busy} onClick={() => void saveQueue(queue.filter((_, j) => j !== i))}>Remove</button></span>
            </div>
          </li>
        ))}</ul>
        {queue.length === 0 && editing === null && <Empty>No upcoming goals. Add one; the teacher and your schedule follow it.</Empty>}
        {editing !== null && (
          <form className="goal-form" onSubmit={(e) => { e.preventDefault(); submit(); }}>
            <h3>{editing >= 0 ? "Edit goal" : "New goal"}</h3>
            <label className="field-block">What is it?<select aria-label="Goal kind" value={g.kind} onChange={(e) => setG({ ...g, kind: e.target.value })}>{data.kinds.map((k) => <option key={k} value={k}>{k}</option>)}</select></label>
            <label className="field-block">Title<input aria-label="Goal title" value={g.title} onChange={(e) => setG({ ...g, title: e.target.value })} placeholder="e.g. Onsite at Datadog" /></label>
            <label className="field-block">Date<input aria-label="Goal date" type="date" value={g.date ?? ""} onChange={(e) => setG({ ...g, date: e.target.value })} /></label>
            {g.kind === "interview" && <label className="field-block">Company<input aria-label="Company" value={g.company ?? ""} onChange={(e) => setG({ ...g, company: e.target.value })} /></label>}
            <label className="field-block">Topics (comma separated)<input aria-label="Topics" value={topics} onChange={(e) => setTopics(e.target.value)} placeholder="e.g. graphs, system design" /></label>
            <label className="field-block">Minutes a day<input aria-label="Minutes a day" type="number" min={10} max={600} value={g.minutes_per_day ?? 90} onChange={(e) => setG({ ...g, minutes_per_day: Number(e.target.value) })} /></label>
            <div className="buttons"><button className="primary" type="submit" disabled={act.busy}>Save and re-plan</button>{queue.length > 0 && <button type="button" onClick={() => setEditing(null)}>Cancel</button>}</div>
          </form>
        )}
        <ActStatus act={act} label="Saving and re-planning…" />
        {msg && <p className="muted small" role="status">{msg}</p>}
        {plan && (
          <details open><summary>Today, re-planned</summary>
            <ul className="rows">{plan.schedule.map((s) => <li key={s.task}><div className="row-head"><span>{s.title}</span><span className="muted small">{s.start.slice(11, 16)}–{s.end.slice(11, 16)}</span></div></li>)}</ul>
          </details>
        )}
      </Section>
      <Section title="Long-term goals" block="long-term">
        <ul className="rows">{lt.map((x, i) => <li key={x.text}><div className="row-head"><span>{x.text}{x.by ? <span className="muted small"> · by {x.by}</span> : null}</span><button type="button" disabled={ltAct.busy} onClick={() => void saveLt(lt.filter((_, j) => j !== i))}>Remove</button></div></li>)}</ul>
        {lt.length === 0 && <Empty>Only you set these. Add the outcomes you are working toward.</Empty>}
        <form className="search" onSubmit={(e) => { e.preventDefault(); if (newLt.trim()) void saveLt([...lt, { text: newLt.trim() }]).then((ok) => ok && setNewLt("")); }}>
          <input aria-label="New long-term goal" value={newLt} onChange={(e) => setNewLt(e.target.value)} placeholder="e.g. Staff engineer by 2027" />
          <button type="submit" disabled={ltAct.busy || !newLt.trim()}>Add</button>
        </form>
        <ActStatus act={ltAct} label="Saving…" />
      </Section>
    </div>
  );
}
