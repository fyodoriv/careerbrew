import { useCallback, useEffect, useRef, useState } from "react";
import { explain } from "./act";
import { ApiError } from "./api";
import { peek, restore, save } from "./cache";
import { CHANGED_EVENT } from "./live";

export interface Loadable<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
  // When `data` came from the page's cache (issue #142): the time it was fetched, and whether new data is known
  // to exist (`/api/changes` moved). Null once a fresh answer arrived.
  cached: { at: number; stale: boolean } | null;
}

// A loader may name its endpoint (`api.today.key`); it keys the page's cache unless `opts.key` says otherwise.
type Loader<T> = (() => Promise<T>) & { key?: string };

// Load data for a view; `reload` refetches after an action (adding a task, toggling the watcher).
// `live` views also refetch, keeping the old data on screen, when the server reports new data.
// With a key, the last answer shows at once (from memory on a view switch, from the tab's encrypted copy on a
// reload) and the fresh one replaces it when it arrives: stale-while-revalidate (issue #142).
export function useData<T>(load: Loader<T>, deps: unknown[] = [], opts: { live?: boolean; key?: string | null } = {}): Loadable<T> {
  const key = opts.key === undefined ? (load.key ?? null) : opts.key;
  const first = key ? peek<T>(key) : undefined;
  const [data, setData] = useState<T | null>(first?.data ?? null);
  const [cached, setCached] = useState<Loadable<T>["cached"]>(first ? { at: first.at, stale: first.stale } : null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  const shownKey = useRef<string | null | undefined>(first ? key : undefined);
  // Loading at once: a Retry click replaces the error with "Loading …" in the same render.
  const reload = useCallback(() => {
    setLoading(true);
    setTick((t) => t + 1);
  }, []);
  const live = !!opts.live;
  useEffect(() => {
    if (!live) return;
    window.addEventListener(CHANGED_EVENT, reload);
    return () => window.removeEventListener(CHANGED_EVENT, reload);
  }, [live, reload]);
  useEffect(() => {
    let alive = true;
    let fresh = false;
    setLoading(true);
    if (key && shownKey.current !== key) {
      // A new endpoint (first mount, or new arguments): show its last answer while the fresh one loads.
      const show = (e: { data: T; at: number; stale: boolean }) => {
        shownKey.current = key;
        setData(e.data);
        setCached({ at: e.at, stale: e.stale });
      };
      const hit = peek<T>(key);
      if (hit) show(hit);
      else void restore<T>(key).then((e) => alive && e && !fresh && show(e));
    }
    load()
      .then((d) => {
        if (!alive) return;
        fresh = true;
        shownKey.current = key;
        setData(d);
        setCached(null);
        setError(null);
        if (key) save(key, d);
      })
      .catch((e: unknown) => alive && setError(e instanceof ApiError && e.status === 403 ? `This view is not available here: ${e.message}` : explain(e)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, key, ...deps]);
  return { data, error, loading, reload, cached };
}
