import type { Act } from "./act";
import { withCode } from "./text";

// Shows "Working…" while an action runs and its error, with the fix, when it fails (issue #57).
export function ActStatus({ act, label = "Working…" }: { act: Pick<Act, "busy" | "error">; label?: string }) {
  return (
    <>
      {act.busy && <span className="muted small working" aria-live="polite">{label}</span>}
      {act.error && <p className="error" role="alert">{withCode(act.error)}</p>}
    </>
  );
}
