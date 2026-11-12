// Live refresh (issue #55): poll the server's change version while the tab is visible,
// and tell live views to refetch when it moves. A move also marks the page's cached views stale (issue #142).
// Lifecycle: browser-page scope; startLiveRefresh returns a stop function that App calls on unmount.
import { api } from "./api";
import { markAllStale } from "./cache";

export const CHANGED_EVENT = "careerbrew:changed";

export function startLiveRefresh(intervalMs = 5000): () => void {
  let last: string | null = null;
  let busy = false;
  let stopped = false;
  const check = async () => {
    if (busy || document.visibilityState !== "visible") return;
    busy = true;
    try {
      const { version } = await api.changes();
      if (stopped) return;
      const changed = last === null || version !== last;
      last = version;
      if (changed) {
        markAllStale();
        window.dispatchEvent(new Event(CHANGED_EVENT));
      }
    } catch {
      // the server may be restarting; try again on the next tick
    } finally {
      busy = false;
    }
  };
  void check();
  const timer = window.setInterval(() => void check(), intervalMs);
  const onVisible = () => void check();
  document.addEventListener("visibilitychange", onVisible);
  return () => {
    stopped = true;
    window.clearInterval(timer);
    document.removeEventListener("visibilitychange", onVisible);
  };
}
