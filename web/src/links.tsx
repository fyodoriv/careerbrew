import type { ReactNode } from "react";
import { webUrl } from "./api";
import type { TaskLink } from "./api";

// Issue #143: a task's links and its own why, on every task row. Only a full http(s) address becomes a link
// (webUrl: never javascript:, data:, or file:), and it opens in a new tab. Text stays text: React escapes it,
// and nothing here sets HTML.

function Out({ href, children }: { href: string; children: ReactNode }) {
  return <a href={href} target="_blank" rel="noreferrer noopener">{children}</a>;
}

// URLs end at a space or a quote; a trailing full stop, comma, or closing bracket belongs to the sentence.
const URLS = /(https?:\/\/[^\s<>"'`]+)/g;
const TRAIL = /[.,;:!?)\]}]+$/;

export function Linkify({ text }: { text: string }) {
  const parts = text.split(URLS);
  return (
    <>
      {parts.map((part, i) => {
        if (i % 2 === 0) return part;
        const tail = part.match(TRAIL)?.[0] ?? "";
        const raw = tail ? part.slice(0, -tail.length) : part;
        const href = webUrl(raw);
        return href ? <span key={i}><Out href={href}>{raw}</Out>{tail}</span> : part;
      })}
    </>
  );
}

export function TaskLinks({ links }: { links?: TaskLink[] }) {
  if (!links?.length) return null;
  return (
    <ul className="task-links small" aria-label="Links">
      {links.map((l, i) => {
        const href = webUrl(l.url);
        const label = l.label || l.url;
        return <li key={`${i}-${l.url}`}>{href ? <Out href={href}>{label}</Out> : <span className="muted">{label === l.url ? label : `${label}: ${l.url}`}</span>}</li>;
      })}
    </ul>
  );
}

// The task's own why (agents put context there), next to the computed score line.
export function TaskWhy({ text }: { text?: string }) {
  if (!text?.trim()) return null;
  return <div className="task-why small"><span className="muted">Context: </span><Linkify text={text} /></div>;
}
