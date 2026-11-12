import { About } from "./about";
import { api } from "./api";
import type { Standup, StandupAction, StandupProof, StandupRow } from "./api";
import { CopyResult } from "./copy";
import { useCopy } from "./useCopy";
import { PAGE_SIZE } from "./order";
import { Paged } from "./pager";
import { slug } from "./paging";
import { timeDone, useOpened } from "./timing";
import { useData } from "./useData";
import { Empty, Section, State } from "./views";
import { FeedHealthNotice } from "./feedHealth";
import { useFeedHealth } from "./useFeedHealth";

// Issue #110: what you did on each row you report on since the last standup. Each row shows a talking point
// first, then its status, then the evidence. Rows you own always show; others only when your work matched them.
// The same report is the page `careerbrew work standup` writes (out/standup.html). Issue #229: an agent's talking
// point replaces a row's generated line and says "written by an agent"; actions an agent passed join the evidence.
// Issue #271: a row with no update is hidden (one count line says how many); with live proofs passed, each row lists
// the proofs it passed, and a row that failed never reaches the view. The window line says when the rows were passed.

function Keys({ keys }: { keys: string[] }) {
  return <>{keys.map((k) => <span key={k} className="tag">{k}</span>)}</>;
}

function Evidence({ actions, empty }: { actions: StandupAction[]; empty: string }) {
  if (!actions.length) return <p className="muted small standup-evidence">{empty}</p>;
  return (
    <ul className="rows standup-evidence">
      {actions.map((a, i) => (
        <li key={`${a.at}-${i}`} className="standup-action" data-kinds={a.kinds.join(" ")}>
          <span className="muted small">{a.when}</span>{" "}
          <span className="tag">{[...new Set(a.nouns)].join(" · ")}</span>{" "}
          {a.url ? <a href={a.url} target="_blank" rel="noreferrer noopener">{a.title}</a> : <span>{a.title}</span>}
          {a.repo && <span className="muted small"> {a.repo}</span>}
          <Keys keys={a.keys} />
          {a.via && <span className="muted small"> via {a.via}</span>}
        </li>
      ))}
    </ul>
  );
}

function Proofs({ proofs }: { proofs: StandupProof[] }) {
  return (
    <ul className="rows standup-proofs">
      {proofs.map((p, i) => (
        <li key={`${p.url}-${i}`} className="standup-proof">
          <span className="muted small">Checked {p.when} in {p.source}:</span>{" "}
          <a href={p.url} target="_blank" rel="noreferrer noopener">{p.quote}</a>
        </li>
      ))}
    </ul>
  );
}

function pointsFor(row: { talking_point: string; talking_points?: string[] }): string[] {
  return row.talking_points?.length ? row.talking_points : [row.talking_point];
}

function TalkingPoints({ row }: { row: { talking_point: string; talking_points?: string[]; agent_written?: boolean } }) {
  const points = pointsFor(row);
  const by = row.agent_written ? <span className="tag standup-by">written by an agent</span> : null;
  return points.length === 1
    ? <p className="standup-talk">{points[0]}{by && <> {by}</>}</p>
    : <><ul className="standup-talk">{points.map((point, i) => <li key={i}>{point}</li>)}</ul>{by}</>;
}

function ReportWindow({ data: d }: { data: Standup }) {
  const browserZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  let reportZone = d.window.tz;
  try {
    reportZone = Intl.DateTimeFormat("en-US", { timeZone: reportZone }).resolvedOptions().timeZone;
  } catch {
    // Python can accept a zone name that the browser does not support.
  }
  return <p className="muted standup-window">
    Since <strong>{d.window.since}</strong> ({d.window.compares} standup) until {d.window.until}{browserZone !== reportZone && <> ({d.window.tz})</>} · {d.counts.actions} action{d.counts.actions === 1 ? "" : "s"} of yours{d.updated && <> · <span className="standup-updated">{d.updated}</span></>}.
  </p>;
}

function Row({ row }: { row: StandupRow }) {
  const proofs = row.proofs ?? [];
  return (
    <Section title={row.title} className={`standup-row${row.mine ? " mine" : ""}`} action={<span><Keys keys={row.keys} /></span>}>
      <TalkingPoints row={row} />
      <p className="muted small standup-status"><b>Status:</b> {row.status || "not given"} · <b>Owner:</b> {row.mine ? "you" : row.owner || "no owner"}</p>
      {(row.actions.length > 0 || !proofs.length) && <Evidence actions={row.actions} empty="No actions of yours since the last standup." />}
      {proofs.length > 0 && <Proofs proofs={proofs} />}
    </Section>
  );
}

// The rows were cleared on purpose (a plugin passed []), or proofs were passed and every row with an update failed them.
function noRowPassed(d: Standup): boolean {
  return Boolean(d.cleared || (d.counts.gated && !d.rows.length && d.counts.failed));
}

// Each row and group keeps its speaking bullets nested under its title on the clipboard.
function standupText(d: Pick<Standup, "rows" | "groups">): string {
  const item = (title: string, row: { talking_point: string; talking_points?: string[] }) => `• ${title}\n${pointsFor(row).map((point) => `  – ${point}`).join("\n")}`;
  return [...d.rows.map((r) => item(r.title, r)), ...d.groups.map((g) => item(g.label, g))].join("\n");
}

export function StandupView() {
  const health = useFeedHealth();
  const q = useData(api.standup, [], { live: true });
  const { copied, copy, clear } = useCopy();
  const text = q.data ? standupText(q.data) : "";
  useOpened("standup", text !== "");
  const copyStandup = () => {
    timeDone("standup", "copy");
    copy(text);
  };
  return (
    <>
    <header className="page-head standup-head">
      <div className="standup-heading"><h1>Standup</h1><button type="button" className="primary" disabled={!text} onClick={copyStandup}>Copy standup</button></div>
      {q.data && <ReportWindow data={q.data} />}
    </header>
    <FeedHealthNotice data={health.data} />
    <State q={q} label="the standup report">
      {(d) => (
        <div className="stack">
          <CopyResult copied={copied} via="slack" onClose={clear} />
          {noRowPassed(d) && <p className="callout" role="status">No row passed the proof check.</p>}
          {!d.has_rows && !noRowPassed(d) && (
            <p className="callout" role="status">
              No rows to report on yet: a plugin or an agent passes them with <code>careerbrew work standup --rows &lt;file.json|-&gt;</code> (title, status, owner, keys). Until then, your work shows under Other work.
            </p>
          )}
          {d.rows.length > 0 && (
            <div className="stack" data-block="rows">
              <header className="block-head"><h2>Items you report on <span className="count">{d.rows.length}</span></h2><About block="rows" /></header>
              <Paged id="rows" rows={d.rows} size={PAGE_SIZE.rows} noun="rows you report on">
                {(rows) => rows.map((r, i) => <Row key={`${r.title}-${i}`} row={r} />)}
              </Paged>
            </div>
          )}
          {d.groups.length > 0 && (
            <div className="stack" data-block="groups">
              <header className="block-head"><h2>Work outside these items</h2><About block="groups" /></header>
              {d.groups.map((g) => (
                <Section key={g.label} title={g.label} className="standup-group">
                  <TalkingPoints row={g} />
                  <Paged id={`group-${slug(g.label)}`} rows={g.actions} size={PAGE_SIZE.group} noun={`${g.label} evidence`}>
                    {(rows) => <Evidence actions={rows} empty="" />}
                  </Paged>
                </Section>
              ))}
            </div>
          )}
          {d.hidden > 0 && <p className="muted small">{d.hidden} row{d.hidden === 1 ? "" : "s"} with no update hidden.</p>}
          {!d.rows.length && !d.groups.length && !noRowPassed(d) && <Empty>No work of yours since the last standup yet.</Empty>}
        </div>
      )}
    </State>
    </>
  );
}
