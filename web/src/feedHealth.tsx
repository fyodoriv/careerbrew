// Health belongs to this mounted browser view. The server owns acquisition age.
import type { FeedHealth, ResultRow, SourceHealthStatus } from "./api";

const messages: Record<SourceHealthStatus, string> = {
  healthy: "Work sources are current.", missing: "Work sources are not set up.",
  stopped: "Work source refresh is stopped.", stale: "Work sources need a fresh pull.",
  error: "The latest work source check failed.", unknown: "Work source freshness is unknown.",
};

function age(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return "unknown age";
  if (seconds < 60) return "less than a minute ago";
  const minutes = Math.floor(seconds / 60);
  return minutes < 60 ? `${minutes} min ago` : `${Math.floor(minutes / 60)} h ago`;
}

function RepairRequest({ status }: { status: SourceHealthStatus }) {
  const request = status === "missing" ? "Set up my Careerbrew work feed." : status === "stopped" ? "Start refreshing my Careerbrew work feed." : "Check and refresh my Careerbrew work sources.";
  return <p>Recent work may be missing from Today and Standup. Ask Claude: “{request}”</p>;
}

export function FeedHealthNotice({ data }: { data: FeedHealth | null }) {
  // A missing/invalid response must not look like a successful source check.
  if (!data || data.version !== 1) return <section className="callout callout-warn feed-health" aria-label="Work source health"><p><strong>Work source freshness is unknown.</strong></p><RepairRequest status="unknown" /></section>;
  const warning = data.status !== "healthy" || data.reason === "optional-gap";
  return <section className={`${warning ? "callout callout-warn" : "callout"} feed-health`} aria-label="Work source health">
    <p><strong>{messages[data.status]}</strong>{data.mode === "direct" && <> Direct batch coverage: {data.healthy} of {data.required} required sources. Adapters {data.running ? "running" : "stopped"}.</>}</p>
    {data.reason === "optional-gap" && <p>An optional source needs attention.</p>}
    {warning && data.sources.length > 0 && <ul>{data.sources.slice(0, 4).map((source) => <li key={source.name}>{source.name}: {source.health}; acquired {age(source.age_seconds)}.</li>)}</ul>}
    {warning && data.sources.length > 4 && <p>{data.sources.length - 4} more sources are included in this check.</p>}
    {warning && <RepairRequest status={data.status} />}
    {data.row_review.status !== "current" && <p>Row facts {data.row_review.status === "due" ? `were supplied ${age(data.row_review.age_seconds)}; review is due` : "have an unknown review time"}. Ask Claude to check the saved row facts before updating them. New activity does not review them.</p>}
  </section>;
}

export function SourceAge({ row, health }: { row: ResultRow; health: FeedHealth | null }) {
  const source = health?.sources?.find((item) => item.name === row.feed_source);
  return <span className="muted small source-age"> Source {source ? `${source.health}; acquired ${age(source.age_seconds)}` : "freshness unknown"}.</span>;
}
