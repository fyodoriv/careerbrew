import { useEffect, useMemo, useState } from "react";
import { explain } from "./act";
import { api } from "./api";
import type { Action, ActionArg, ActionResult } from "./api";
import { friendlyDay } from "./dates";
import { withCode } from "./text";
import { useData } from "./useData";
import { CachedNote, Empty, Section, State } from "./views";

// Every careerbrew action, generated from the CLI: pick one, fill its form, run it. The CLI and agents run the same list.

function Field({ arg, value, set }: { arg: ActionArg; value: unknown; set: (v: unknown) => void }) {
  const label = `${arg.name.replace(/_/g, " ")}${arg.required ? " (required)" : ""}`;
  if (arg.kind === "flag") {
    return <label className="field"><input type="checkbox" checked={Boolean(value)} onChange={(e) => set(e.target.checked)} /> {label}</label>;
  }
  if (arg.kind === "choice" && arg.choices) {
    return (
      <label className="field">{label}
        <select aria-label={arg.name} value={String(value ?? "")} onChange={(e) => set(e.target.value)}>
          <option value="">—</option>
          {arg.choices.map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
      </label>
    );
  }
  return (
    <label className="field">{label}
      <input aria-label={arg.name} type={arg.kind === "int" || arg.kind === "number" ? "number" : "text"} value={String(value ?? "")} placeholder={arg.many ? "comma-separated" : arg.help}
        onChange={(e) => set(arg.many ? e.target.value.split(",").map((x) => x.trim()).filter(Boolean) : e.target.value)} />
    </label>
  );
}

function Runner({ action }: { action: Action }) {
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [confirm, setConfirm] = useState(false);
  const [result, setResult] = useState<ActionResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [promiseChoices, setPromiseChoices] = useState<{ id: string; label: string }[] | null>(null);
  const [promiseNotes, setPromiseNotes] = useState<string[]>([]);
  const [promiseError, setPromiseError] = useState("");
  useEffect(() => {
    if (action.name !== "work.promise.accept") return;
    let active = true;
    void api.runAction("work.promise.outcomes", { json: true }).then((result) => {
      if (!active) return;
      if (result.exit !== 0) throw new Error("Could not load promise outcomes.");
      const rows: unknown = JSON.parse(result.output)?.outcomes;
      if (!Array.isArray(rows)) throw new Error("Could not load promise outcomes.");
      const choices: { id: string; label: string }[] = [];
      const notes: string[] = [];
      for (const row of rows) {
        if (!row || typeof row.promise !== "string") continue;
        if (row.state === "decision-needs-review" && typeof row.prior_state === "string" && typeof row.current_state === "string") {
          const priorDue = typeof row.prior_due === "string" && row.prior_due ? `, due ${friendlyDay(row.prior_due)}` : "";
          const currentDue = typeof row.current_due === "string" && row.current_due ? `, due ${friendlyDay(row.current_due)}` : "";
          notes.push(`${row.promise}: prior ${row.prior_state}${priorDue}; current ${row.current_state}${currentDue}.`);
        }
        if (typeof row.id === "string" && row.id && (row.state === "done" || row.state === "moved")) {
          choices.push({ id: row.id, label: `${row.promise}: ${row.state === "done" ? "Mark done" : `Move${row.due ? `, due ${friendlyDay(row.due)}` : ""}`}` });
        }
        if (Array.isArray(row.resolutions)) for (const option of row.resolutions) {
          if (!option || typeof option.id !== "string" || !option.id) continue;
          const label = option.choice === "keep" ? `Keep the prior decision${option.due ? `, due ${friendlyDay(option.due)}` : ""}` : option.choice === "reopen" ? `Reopen${option.due ? `, due ${friendlyDay(option.due)}` : ""}` : option.choice === "drop" ? "Mark dropped" : "";
          const separate = option.separate_due ? `; ${option.separate_existing ? "keep" : "create"} a separate promise due ${friendlyDay(option.separate_due)}` : "";
          if (label) choices.push({ id: option.id, label: `${row.promise}: ${label}${separate}` });
        }
      }
      setPromiseNotes(notes);
      setPromiseChoices(choices);
    }).catch(() => { if (active) setPromiseError("Could not load promise outcomes. Try again."); });
    return () => { active = false; };
  }, [action.name]);
  if (action.terminal_only) {
    return <p className="callout">Terminal only: {action.terminal_only}. Run <code>{action.command}</code> in your terminal.</p>;
  }
  const run = async () => {
    setBusy(true);
    try {
      setResult(await api.runAction(action.name, values, confirm));
      setError(null);
    } catch (e) {
      setError(explain(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="stack" onSubmit={(e) => { e.preventDefault(); void run(); }} aria-label={`Run ${action.name}`}>
      <p className="muted small">Same as <code>{action.command}</code>{action.help ? ` — ${action.help}` : ""}</p>
      {action.name === "work.promise.accept" && (promiseError ? <p className="error" role="alert">{promiseError}</p> : promiseChoices === null ? <p>Loading promise outcomes…</p> : <>{promiseNotes.map((note) => <p key={note}>{note}</p>)}{promiseChoices.length ? <fieldset className="stack"><legend>Choose an outcome</legend>{promiseChoices.map((choice) => <label className="field" key={choice.id}><input type="radio" name="promise-outcome" value={choice.id} checked={values.proposal_id === choice.id} onChange={() => { setValues({ ...values, proposal_id: choice.id }); setConfirm(false); }} /> {choice.label}</label>)}</fieldset> : <p>No current promise decisions to review.</p>}</>)}
      {action.args.filter((a) => !((action.name === "work.promise.accept" && a.name === "proposal_id") || (action.confirm && a.name === "yes" && a.flag === "--yes" && a.kind === "flag"))).map((a) => <Field key={a.name} arg={a} value={values[a.name]} set={(v) => setValues({ ...values, [a.name]: v })} />)}
      {action.confirm && <label className="field"><input type="checkbox" checked={confirm} onChange={(e) => setConfirm(e.target.checked)} /> I understand: it {action.confirm}</label>}
      <div className="quick-add"><button type="submit" disabled={busy || (Boolean(action.confirm) && !confirm) || (action.name === "work.promise.accept" && !values.proposal_id)}>Run</button>{busy && <span className="muted small working" aria-live="polite">Running…</span>}</div>
      {error && <p className="error" role="alert">{withCode(error)}</p>}
      {result && (
        <div aria-label="Result">
          <p className="muted small">exit {result.exit} · <code>{result.argv.join(" ")}</code></p>
          <pre className="page">{result.output || "(no output)"}</pre>
        </div>
      )}
    </form>
  );
}

export function ActionsView({ name }: { name?: string }) {
  const q = useData(api.actions);
  const [filter, setFilter] = useState("");
  const [picked, setPicked] = useState<string | undefined>(name);
  const list = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return (q.data?.actions ?? []).filter((a) => !needle || a.name.toLowerCase().includes(needle) || a.help.toLowerCase().includes(needle));
  }, [q.data, filter]);
  if (!q.data) return <State q={q} label="actions">{() => null}</State>;
  const current = q.data.actions.find((a) => a.name === picked);
  return (
    <div className="split">
      <Section title={`Actions (${q.data.actions.length})`} block="list">
        <CachedNote q={q} />
        <input className="filter" aria-label="Filter actions" placeholder="Filter, e.g. day or backup" value={filter} onChange={(e) => setFilter(e.target.value)} />
        {list.length ? (
          <ul className="rows">{list.map((a) => (
            <li key={a.name}><button type="button" className="link" aria-current={a.name === picked ? "true" : undefined} onClick={() => setPicked(a.name)}>{a.name}</button>
              <span className="muted small"> {a.help}{a.terminal_only ? " · terminal only" : ""}{a.confirm ? " · asks to confirm" : ""}</span></li>
          ))}</ul>
        ) : <Empty>No action matches.</Empty>}
      </Section>
      <Section title={current ? current.name : "Pick an action"} block="runner">
        {current ? <Runner key={current.name} action={current} /> : <Empty>Choose one on the left. Agents run the same actions with <code>careerbrew actions --json</code>.</Empty>}
      </Section>
    </div>
  );
}
