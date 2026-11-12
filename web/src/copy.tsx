import { useEffect, useRef } from "react";

// What a copy did (the PM update, issue #145; the standup, US-86): a toast, or the text selected to copy by hand when the browser refuses.

export function CopyResult({ copied, via, onClose }: { copied: { ok: boolean; text: string } | null; via: string; onClose: () => void }) {
  const box = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (copied && !copied.ok) box.current?.select();
  }, [copied]);
  if (!copied) return null;
  if (copied.ok) return <div className="toast" role="status">Copied, paste it in {via === "email" ? "your email" : "Slack"}</div>;
  return (
    <div className="manual-copy">
      <p className="callout callout-warn small" role="alert">The browser did not allow the copy. The text below is selected: press Cmd+C (Ctrl+C), then paste it.</p>
      <textarea ref={box} readOnly aria-label="Update to copy" className="pm-body" rows={Math.min(10, copied.text.split("\n").length + 1)} value={copied.text} onFocus={(e) => e.currentTarget.select()} />
      <button type="button" className="secondary" onClick={onClose}>Close</button>
    </div>
  );
}

