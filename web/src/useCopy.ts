import { useEffect, useState } from "react";

const TOAST_MS = 4000;

// Copy to the clipboard inside the click handler (the browser needs the click as the user's gesture).
// When the browser refuses, the text is shown selected instead, so Cmd+C or Ctrl+C copies it.
export function useCopy() {
  const [copied, setCopied] = useState<{ ok: boolean; text: string } | null>(null);
  useEffect(() => {
    if (!copied?.ok) return;
    const t = setTimeout(() => setCopied(null), TOAST_MS);
    return () => clearTimeout(t);
  }, [copied]);
  const copy = (text: string, html?: string) => {
    const done = (ok: boolean) => setCopied({ ok, text });
    const writePlain = () => {
      try {
        navigator.clipboard.writeText(text).then(() => done(true), () => done(false));
      } catch {
        done(false);
      }
    };
    try {
      if (html && typeof ClipboardItem === "function" && typeof navigator.clipboard?.write === "function") {
        navigator.clipboard.write([new ClipboardItem({
          "text/plain": new Blob([text], { type: "text/plain" }),
          "text/html": new Blob([html], { type: "text/html" }),
        })]).then(() => done(true), writePlain);
        return;
      }
    } catch {
      // Some browsers refuse rich writes. Try the same plain fallback.
    }
    writePlain();
  };
  return { copied, copy, clear: () => setCopied(null) };
}
