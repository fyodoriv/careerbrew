import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { ABOUT, PAGE_SIZE, VIEW_BLOCKS } from "./order";
import { isBare, VIEWS } from "./router";

// Issue #146: docs/web.md ("Block order and paging") and VIEW_BLOCKS list the same blocks in the same order, and
// the same rows per page. The browser test (web/e2e/order-paging.spec.ts) checks each view's page against VIEW_BLOCKS.

const doc = readFileSync(resolve(process.cwd(), "../docs/web.md"), "utf-8");  // vitest runs in web/
const section = doc.split("## Block order and paging")[1]?.split("\n## ")[0] ?? "";
const rows = new Map(
  section.split("\n").filter((l) => l.startsWith("| ") && !l.startsWith("| View") && !l.startsWith("|---")).map((l) => {
    const [, view, blocks, paged] = l.split("|").map((c) => c.trim());
    return [view, { blocks, paged }] as const;
  }),
);
const ids = (cell: string) => [...cell.matchAll(/`([a-z][a-z0-9-]*)`/g)].map((m) => m[1]);

describe("block order (issue #146)", () => {
  it("names every view once, by its sidebar label", () => {
    expect([...rows.keys()].sort()).toEqual(VIEWS.map((v) => v.label).sort());
    expect(Object.keys(VIEW_BLOCKS).sort()).toEqual(VIEWS.map((v) => v.id).sort());
  });

  for (const v of VIEWS) {
    it(`${v.label}: docs/web.md lists the blocks of VIEW_BLOCKS, in its order`, () => {
      expect(ids(rows.get(v.label)?.blocks ?? "")).toEqual([...VIEW_BLOCKS[v.id]]);
    });
  }

  it("block ids are bare tokens, unique on their view", () => {
    for (const blocks of Object.values(VIEW_BLOCKS)) {
      expect(new Set(blocks).size).toBe(blocks.length);
      for (const b of blocks) expect(isBare(b)).toBe(true);
    }
  });

  it("docs/web.md gives each paged list the rows per page of PAGE_SIZE", () => {
    const documented = Object.fromEntries([...rows.values()].flatMap((r) => [...r.paged.matchAll(/`([a-z][a-z0-9-]*)` (\d+)/g)].map((m) => [m[1], Number(m[2])])));
    expect(documented).toEqual({ ...PAGE_SIZE });
  });

  it("every block says what it is (issue #147): one answer per block id, and none for a block that does not exist", () => {
    const blocks = new Set(Object.values(VIEW_BLOCKS).flat());
    for (const id of blocks) expect(ABOUT[id]?.trim(), `${id}: add its "What is this?" answer to ABOUT`).toBeTruthy();
    expect(Object.keys(ABOUT).filter((k) => !blocks.has(k)), "ABOUT names blocks no view has").toEqual([]);
    for (const text of Object.values(ABOUT)) expect(text.length, text).toBeLessThanOrEqual(140);
  });
});
