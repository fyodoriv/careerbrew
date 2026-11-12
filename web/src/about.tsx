import { useState } from "react";
import { ABOUT } from "./order";

// Issue #147: a block's "What is this?" line. The answer (ABOUT in order.ts) renders only while the line is open, so a
// folded one adds no words to the page.
export function About({ block }: { block: string }) {
  const [open, setOpen] = useState(false);
  const text = ABOUT[block];
  if (!text) return null;
  return (
    <details className="about" data-about={block} onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>What is this?</summary>
      {open && <p className="muted small">{text}</p>}
    </details>
  );
}
