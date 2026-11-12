import { useCallback, useRef, useState } from "react";
import { ApiError } from "./api";

// Shared feedback for every button, form, and inline answer (issue #57):
// a "Working…" status while the request runs, and on failure an error that names the fix.
// State scope: component-instance (one per mounted control).

export function explain(e: unknown): string {
  if (e instanceof ApiError) {
    const what = e.message || `request failed (${e.status})`;
    if (e.status === 401) return `${what}. Fix: open the app again with \`uv run careerbrew open\`; the link changes on every start.`;
    if (e.status === 403) return `${what}. Fix: this action is off on this machine; see Setup for the machine role and policy.`;
    if (e.status === 404) return `${what}. Fix: it no longer exists; reload the page to see the current list.`;
    if (e.status >= 500) return `${what}. Fix: careerbrew hit an error; check the terminal that runs \`careerbrew serve\`, run \`scripts/doctor.sh\`, then try again.`;
    return `${what}. Fix: check what you entered, then try again.`;
  }
  if (e instanceof TypeError) return "careerbrew is not answering. Fix: start it with `uv run careerbrew`, then try again.";
  return `${e instanceof Error ? e.message : String(e)}. Fix: reload the page and try again.`;
}

export interface Act {
  busy: boolean;
  error: string | null;
  run: <T>(fn: () => Promise<T>) => Promise<T | undefined>;
  clear: () => void;
}

export function useAct(): Act {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const running = useRef(false);
  const run = useCallback(async <T>(fn: () => Promise<T>): Promise<T | undefined> => {
    if (running.current) return undefined; // one request at a time, even on a double click
    running.current = true;
    setBusy(true);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      setError(explain(e));
      return undefined;
    } finally {
      running.current = false;
      setBusy(false);
    }
  }, []);
  const clear = useCallback(() => setError(null), []);
  return { busy, error, run, clear };
}

