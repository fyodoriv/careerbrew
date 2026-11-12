import { webcrypto } from "node:crypto";
import { afterEach, describe, expect, it, vi } from "vitest";

// The tab's encrypted copy (issue #142) asks the server for its key again after a failed /api/tab-key call.

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

const stored = () => Array.from({ length: sessionStorage.length }, (_, i) => sessionStorage.key(i) ?? "").filter((k) => k.startsWith("careerbrew-cache:"));
const newKey = () => Buffer.from(webcrypto.getRandomValues(new Uint8Array(32))).toString("base64");
const settle = () => new Promise((r) => setTimeout(r, 50));

// Answers /api/tab-key in order from `answers` (a key, null for web_tab_cache off, or an HTTP status to fail with).
function server(answers: (string | null | { status: number })[]) {
  vi.stubGlobal("crypto", webcrypto);
  const fetch = vi.fn(async () => {
    const a = answers.shift();
    if (typeof a === "object" && a !== null) return new Response(JSON.stringify({ error: "synthetic failure" }), { status: a.status });
    return new Response(JSON.stringify({ key: a }));
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

describe("the tab key (tab-key-retry)", () => {
  it("is asked for again after a failed call, so the tab keeps its copy without a reload", async () => {
    const fetch = server([{ status: 401 }, newKey()]);  // a 401 right after `careerbrew web rotate-token`, then a fresh sign-in
    const page = await import("./cache");
    page.save("/api/today", { n: 1 });
    await settle();
    expect(stored()).toHaveLength(0);  // no key yet: memory only
    page.save("/api/today", { n: 2 });
    await vi.waitFor(() => expect(stored()).toHaveLength(1));
    expect(fetch).toHaveBeenCalledTimes(2);
    expect((await page.restore("/api/today"))?.data).toEqual({ n: 2 });  // read back from sessionStorage
  });

  it("is asked for once when the server keeps nothing in the tab (web_tab_cache off)", async () => {
    const fetch = server([null, null]);
    const page = await import("./cache");
    page.save("/api/today", { n: 1 });
    await settle();
    page.save("/api/work", { n: 2 });
    await settle();
    expect(await page.restore("/api/today")).toBeUndefined();
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(stored()).toHaveLength(0);
  });
});
