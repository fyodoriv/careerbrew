import type { Standup } from "../src/api";
import { TODAY, open, patch, region, shot, test } from "./helpers";

// US-95: with live proofs passed (`work standup --proof`), a row lists each proof it passed under it, and a row that
// failed never reaches the view; when none passed, the view says so. The seed passes no proofs, so the answer is patched.
const proof = { source: "github", url: "https://github.example.com/acme/shop/pull/12", checked_at: `${TODAY}T08:50:00`, quote: "Merged: add the order export", when: "08:50" };

test("US-95: a row with the live proof it passed", async ({ page }) => {
  await patch<Standup>(page, "**/api/standup", (d) => ({ ...d, rows: d.rows.filter((r) => r.title === "Order export").map((r) => ({ ...r, proofs: [proof] })), counts: { ...d.counts, gated: true, passed: 1, failed: 3 } }));
  await open(page, "standup");
  await shot(region(page, "Order export"), "proved-row");
});

test("US-95: no row passed the proof check", async ({ page }) => {
  await patch<Standup>(page, "**/api/standup", (d) => ({ ...d, rows: [], groups: [], hidden: 0, counts: { ...d.counts, gated: true, passed: 0, failed: 4 } }));
  await open(page, "standup");
  await shot(page.locator("main .view"), "no-row-passed");
});
