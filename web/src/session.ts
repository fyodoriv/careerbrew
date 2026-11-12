// Sign-in state for the app (issue #141): the token lasts across restarts, and the server keeps it in an
// HttpOnly cookie, so a reload, a new tab, or a bookmark signs in by itself.
import { useCallback, useEffect, useRef, useState } from "react";
import { hasToken, keepSignIn, SIGNED_OUT_EVENT, signIn, type SignIn } from "./api";

export type Session = SignIn | "checking";

// Lifecycle: browser-page scope (App's state). A tab that holds the token renders at once and asks for the
// cookie; one without it signs in from the cookie first. A 401 later (rotate-token, expiry) signs in from
// the cookie again, else shows the sign-in page. `epoch` moves on each sign-in, so views refetch with the token.
export function useSession(): { state: Session; epoch: number; retry: () => void } {
  const [state, setState] = useState<Session>(() => (hasToken() ? "ok" : "checking"));
  const [epoch, setEpoch] = useState(0);
  const busy = useRef(false);
  const retry = useCallback(() => {
    if (busy.current) return;
    busy.current = true;
    signIn()
      .then((s) => {
        setState(s);
        if (s === "ok") setEpoch((e) => e + 1);
      })
      .finally(() => {
        busy.current = false;
      });
  }, []);
  useEffect(() => {
    if (hasToken()) keepSignIn().catch(() => undefined);  // the server may be restarting; the next start keeps the token
    else retry();
  }, [retry]);
  useEffect(() => {
    window.addEventListener(SIGNED_OUT_EVENT, retry);
    return () => window.removeEventListener(SIGNED_OUT_EVENT, retry);
  }, [retry]);
  return { state, epoch, retry };
}
