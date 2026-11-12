// Each mounted browser view owns its health refresh and cleanup.
import { useEffect } from "react";
import { api } from "./api";
import { useData } from "./useData";

export function useFeedHealth() {
  const q = useData(api.feedHealth, [], { live: true, key: null });
  const { data, loading, error, reload } = q;
  useEffect(() => {
    // Server-relative delay avoids trusting the browser's wall clock. A visible
    // one-minute bound also covers new row-review and acquisition deadlines.
    const until = data?.next_expiry ? Date.parse(data.next_expiry) - Date.parse(data.as_of) : 60000;
    const delay = Number.isFinite(until) ? Math.min(60000, Math.max(1000, until)) : 60000;
    const timer = document.visibilityState === "visible" && !loading ? window.setTimeout(reload, delay) : null;
    const visible = () => {
      if (document.visibilityState === "visible") reload();
      else if (timer !== null) window.clearTimeout(timer);
    };
    document.addEventListener("visibilitychange", visible);
    return () => {
      if (timer !== null) window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [data, reload, loading, error]);
  return { ...q, data: error ? null : data };
}
