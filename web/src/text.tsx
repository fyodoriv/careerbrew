import type { ReactNode } from "react";
import { friendlyDay } from "./dates";

// A date (2026-10-21) never breaks at its hyphens in a narrow column: "2026-10-" then "21" read as two values (US-75).
// Nor does a time range, "(09:00–09:30)" (a Dashboard title on a phone, US-76). A date shows as a friendly day,
// "Wed Oct 21" (US-76: no ISO date in the UI); a weekday already in front of it ("planned for Tue 2026-10-20") is
// not said twice.
export function keepDates(text: string | null | undefined): ReactNode {
  if (!text) return text ?? null;
  const parts = text.split(/((?:\b(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun) )?\d{4}-\d{2}-\d{2}(?![\dT])|\(\d{2}:\d{2}–\d{2}:\d{2}\))/);
  if (parts.length === 1) return text;
  return parts.map((part, i) => {
    if (!(i % 2)) return part;
    const day = /\d{4}-\d{2}-\d{2}/.exec(part);
    return <span key={i} className="nowrap">{day ? friendlyDay(day[0]) : part}</span>;
  });
}

// `code` in a message (a fix, a hint, a pivot action) shows as code, not as backticks.
export function withCode(text: string): ReactNode {
  return text.split(/`([^`]+)`/).map((part, i) => (i % 2 ? <code key={i}>{part}</code> : part));
}
