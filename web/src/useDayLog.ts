import { useState } from "react";
import { api } from "./api";
import type { DayLogView, InboxView } from "./api";
import { useData } from "./useData";
import type { Loadable } from "./useData";

export interface DayLogData {
  log: Loadable<DayLogView>;
  items: Loadable<InboxView>;
  onDone: (v: DayLogView) => void;
}

// Today's day-log blocks (the PM update, questions, suggestions, the log, earlier days) share two reads, made at
// once (issue #142): the inbox (fast) and the day log (it rebuilds the log). An answer, Accept, or Dismiss returns
// the whole day log, which then stands for both until the view unmounts.
export function useDayLog(): DayLogData {
  const q = useData(api.daylog, [], { live: true });
  const iq = useData(api.inboxItems, [], { live: true });
  const [override, setOverride] = useState<DayLogView | null>(null);
  return {
    log: { ...q, data: override ?? q.data },
    items: { ...iq, data: override ? { inbox: override.inbox, groups: override.groups, expired: override.expired } : iq.data },
    onDone: setOverride,
  };
}
