import { api, localDate, localStamp, webUrl } from "./api";
import type { AgentRun, AgentStatus } from "./api";
import { friendlyDay } from "./dates";
import { PAGE_SIZE } from "./order";
import { Paged } from "./pager";
import { useData } from "./useData";
import { Empty, Section, State } from "./views";

// Issue #272: Agent activity on Work. Background agents report each step with `careerbrew work agent event`; the
// rows land under work/, so `/api/changes` moves and this live block refetches within one poll (5 s). Runs are
// grouped by project; each shows its state, its last event, and its link.

const TAG: Record<AgentStatus, string> = { running: "tag tag-rank", quiet: "tag", done: "tag tag-yes", failed: "tag tag-no" };
const when = (iso: string) => `${friendlyDay(localDate(iso))} ${localStamp(iso).slice(11)}`;

function RunRow({ r, quietMinutes }: { r: AgentRun; quietMinutes: number }) {
  const url = webUrl(r.url);
  return (
    <li data-agent={r.agent} data-status={r.status}>
      <div className="row-head">
        <span><span className={TAG[r.status]}>{r.status}</span> <strong>{r.agent}</strong>{r.run !== r.agent && <span className="muted"> · {r.run}</span>}</span>
        <span className="muted small nowrap">last {when(r.at)}</span>
      </div>
      <div>{url ? <a href={url} target="_blank" rel="noreferrer">{r.title}</a> : r.title}</div>
      <div className="muted small">
        {r.events} event{r.events === 1 ? "" : "s"} since {when(r.started_at)}
        {r.status === "quiet" && <> · no event for over {quietMinutes} min: the agent may have stopped</>}
      </div>
    </li>
  );
}

export function AgentActivity() {
  const q = useData(api.agents, [], { live: true });
  return (
    <Section title="Agent activity" block="agents" action={<a className="small" href="#/actions/work.agent.event">Report a step</a>}>
      <State q={q} label="agent activity">
        {(d) => d.groups?.length && d.counts ? (
          <>
            <p className="chips small">
              <span>{d.counts.running} running</span> <span>{d.counts.quiet} quiet</span> <span>{d.counts.done} done</span> <span>{d.counts.failed} failed</span>
            </p>
            <Paged id="agents" rows={d.groups} size={PAGE_SIZE.agents} noun="projects with agents">
              {(groups) => groups.map((g) => (
                <section key={g.project} className="agent-group" aria-label={`Agents: ${g.project || "no project"}`}>
                  <h3>{g.project || <span className="muted">No project</span>}</h3>
                  <ul className="rows">{g.runs.map((r) => <RunRow key={`${r.agent}:${r.run}`} r={r} quietMinutes={d.quiet_minutes} />)}</ul>
                </section>
              ))}
            </Paged>
          </>
        ) : (
          <Empty>
            No agent has reported yet. An agent reports each step as it works, never in a batch at the end:{" "}
            <code>careerbrew work agent event &lt;agent&gt; &quot;&lt;what it did&gt;&quot; --project &lt;name&gt; --url &lt;link&gt;</code>; <code>--status done</code> ends the run.
          </Empty>
        )}
      </State>
    </Section>
  );
}
