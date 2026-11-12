// The sign-in page (issue #141): it shows only when a tab cannot sign in by itself: no link and no
// cookie yet, or the token was rotated or expired. It names the one command that fixes it.
import { useEffect, useState } from "react";
import { openSignedInTab } from "./api";

export function SignInPage({ state, view, onRetry }: { state: "signed-out" | "offline"; view: string; onRetry: () => void }) {
  const [note, setNote] = useState<string | null>(null);
  const [waiting, setWaiting] = useState(false);
  useEffect(() => {
    if (!waiting) return;
    let tries = 0;
    const timer = window.setInterval(() => {  // the new tab sets the cookie; then this tab signs in from it
      tries += 1;
      onRetry();
      if (tries >= 15) setWaiting(false);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [waiting, onRetry]);
  const open = async () => {
    setNote(null);
    const error = await openSignedInTab(view);
    if (error) return setNote(error);
    setNote("A signed-in tab opened in your default browser. This tab signs in again in a moment.");
    setWaiting(true);
  };
  const offline = state === "offline";
  return (
    <section className="card signin" aria-labelledby="signin-title">
      <h2 id="signin-title">{offline ? "careerbrew does not answer" : "Sign in to careerbrew"}</h2>
      <p>{offline ? "The web app does not run on this port, or it is restarting." : "This tab is not signed in: it was opened without a careerbrew link, this browser has no careerbrew sign-in yet, or the token was rotated or expired."}</p>
      <p>Fix: run <code>careerbrew open</code> in a terminal.</p>
      <div className="buttons">
        {!offline && <button type="button" className="primary" onClick={() => void open()}>Open a signed-in tab</button>}
        <button type="button" onClick={onRetry}>Try again</button>
      </div>
      {note && <p role="status" className="muted">{note}</p>}
    </section>
  );
}
