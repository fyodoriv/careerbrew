import AxeBuilder from "@axe-core/playwright";
import { expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import { test } from "./comfort-fixtures";
import { writeFileSync, mkdirSync } from "node:fs";
import { OUT, TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";
import { VIEWS } from "../src/router";

// The comfort budget (goal 2026-10-22, the ui-ux-rankings comfort patterns): every view, on a phone and a desktop,
// in light and dark, with reduced motion, after it has loaded:
//   0 serious or critical axe problems, 0 text under 14 px, 0 text that spills out of its box, 0 sideways scroll,
//   and on a phone 0 touch targets under 44 px.
// It only reads, so it can run anywhere in the suite. Each run writes what it found and a screenshot per view to
// test-results/comfort/<mode>/ (the before/after pairs of a design change come from there).

// Each page waits for its own marker after the hash changes, so the checks never read the view before it.
// The seed is a personal machine: on a corporate one a blocked view keeps its title and h1 but not its content.
type Marker = (page: Page) => Promise<void>;
const titled = (label: string): Marker => async (page) => {
  await expect(page).toHaveTitle(`${label} · careerbrew`);
  await expect(page.locator("main h1")).toHaveText(label);
};
const PAGES: { name: string; hash: string; ready: Marker }[] = [
  ...VIEWS.map((v) => ({ name: v.id, hash: `#/${v.id}`, ready: titled(v.label) })),
  { name: "company", hash: "#/interviews/Datadog", ready: async (page) => { await expect(page.getByRole("heading", { name: "Interview loop: Datadog" })).toBeVisible(); } },
  { name: "action-form", hash: "#/actions/work.task", ready: async (page) => { await expect(page.getByRole("button", { name: "Run", exact: true })).toBeVisible(); } },
  { name: "repair-form", hash: "#/actions/day.start", ready: async (page) => {
    await page.getByLabel("Filter actions").fill("day.start");
    await page.getByRole("button", { name: "day.start", exact: true }).click();
    await expect(page.getByRole("form", { name: "Run day.start" })).toBeVisible();
  } },
];

const MIN_TEXT = 14;
const MIN_TARGET = 44;

const MODES = [
  { name: "phone-light", width: 375, height: 812, scheme: "light" as const, touch: true },
  { name: "phone-dark", width: 375, height: 812, scheme: "dark" as const, touch: true },
  { name: "desktop-light", width: 1280, height: 900, scheme: "light" as const, touch: false },
  { name: "desktop-dark", width: 1280, height: 900, scheme: "dark" as const, touch: false },
];

async function settle(page: Page, p: (typeof PAGES)[number]) {
  await page.evaluate((h) => { window.location.hash = h; }, p.hash);
  await p.ready(page);
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await expect(page.getByText("refreshing…")).toHaveCount(0);
  await page.evaluate(() => document.fonts.ready);
}

// Visible text smaller than MIN_TEXT, one line per distinct element kind (tag.class) with a sample.
async function smallText(page: Page): Promise<string[]> {
  return page.evaluate((min) => {
    const seen = new Map<string, string>();
    for (const el of document.querySelectorAll<HTMLElement>("body *")) {
      const own = [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent!.trim());
      if (!own || !el.getClientRects().length) continue;
      const cs = getComputedStyle(el);
      if (cs.visibility === "hidden" || cs.opacity === "0" || el.closest("[aria-hidden=true], .sr-only")) continue;
      const size = parseFloat(cs.fontSize);
      if (size >= min) continue;
      const key = `${el.tagName.toLowerCase()}${el.className && typeof el.className === "string" ? "." + el.className.trim().split(/\s+/).join(".") : ""} ${size}px`;
      if (!seen.has(key)) seen.set(key, el.textContent!.trim().slice(0, 40));
    }
    // form controls hold their text in a value, not a child node (and iOS zooms into an input under 16 px)
    for (const el of document.querySelectorAll<HTMLElement>("input:not([type=checkbox]):not([type=radio]):not([type=hidden]), select, textarea")) {
      const size = parseFloat(getComputedStyle(el).fontSize);
      if (el.getClientRects().length && size < min) seen.set(`${el.tagName.toLowerCase()} ${size}px`, el.getAttribute("aria-label") ?? "");
    }
    return [...seen].map(([k, v]) => `${k} "${v}"`);
  }, MIN_TEXT);
}

// Controls whose tap area is under MIN_TARGET in either direction. A checkbox or radio counts its label's box.
async function smallTargets(page: Page): Promise<string[]> {
  return page.evaluate((min) => {
    const out = new Set<string>();
    const sel = "a[href], button, input:not([type=hidden]), select, textarea, summary, [role=button], [role=tab], [tabindex='0']";
    for (const el of document.querySelectorAll<HTMLElement>(sel)) {
      if (!el.getClientRects().length || el.closest("[aria-hidden=true], [inert]")) continue;
      const box = (el.matches("input[type=checkbox], input[type=radio]") && el.closest("label")) || el;
      const r = box.getBoundingClientRect();
      // a link inside a sentence needs the height, not the width (WCAG 2.5.8 exempts targets in a sentence)
      const inSentence = el.matches("p a, dd a, .callout a") && [...el.parentElement!.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent!.trim());
      if ((r.width >= min || inSentence) && r.height >= min) continue;
      const name = (el.getAttribute("aria-label") || el.textContent || (el as HTMLInputElement).placeholder || el.tagName).trim().slice(0, 30);
      out.add(`${el.tagName.toLowerCase()} "${name}" ${Math.round(r.width)}x${Math.round(r.height)}`);
    }
    return [...out];
  }, MIN_TARGET);
}

// Text wider than its own box: it spills over its neighbours without scrolling, so the sideways check misses it
// (the blind judge found "goal:promotion" over the Week columns). A number cell that breaks a number counts too.
async function spills(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const out = new Set<string>();
    for (const el of document.querySelectorAll<HTMLElement>("main *")) {
      const own = [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent!.trim());
      if (!own || !el.getClientRects().length || getComputedStyle(el).display === "inline" || el.closest(".sr-only")) continue;
      const what = `${el.tagName.toLowerCase()}${typeof el.className === "string" && el.className ? "." + el.className.trim().split(/\s+/).join(".") : ""}`;
      if (el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).textOverflow !== "ellipsis") out.add(`${what} "${el.textContent!.trim().slice(0, 30)}" spills ${el.scrollWidth - el.clientWidth}px`);
      if (el.matches("td.num")) {  // a number on more than one line
        const range = document.createRange();
        range.selectNodeContents(el);
        const lines = new Set([...range.getClientRects()].filter((r) => r.width).map((r) => Math.round(r.top)));
        if (lines.size > 1) out.add(`${what} "${el.textContent!.trim()}" breaks a number`);
      }
    }
    // an inline element (a link, code, an id) wider than the block it sits in
    for (const el of document.querySelectorAll<HTMLElement>("main *")) {
      if (!/^inline/.test(getComputedStyle(el).display) || !el.textContent!.trim() || el.closest(".sr-only")) continue;
      let box = el.parentElement!;
      while (getComputedStyle(box).display === "inline" && box.parentElement) box = box.parentElement;
      const r = el.getBoundingClientRect(), b = box.getBoundingClientRect();
      if (!r.width || r.right <= b.right + 1) continue;
      const ox = getComputedStyle(box).overflowX;
      if (ox === "visible") out.add(`${el.tagName.toLowerCase()} "${el.textContent!.trim().slice(0, 30)}" spills past its box`);
      else if ((ox === "hidden" || ox === "clip") && getComputedStyle(box).textOverflow !== "ellipsis") out.add(`${el.tagName.toLowerCase()} "${el.textContent!.trim().slice(0, 30)}" is cut off by its box`);
    }
    return [...out];
  });
}

// A raw ISO date the owner reads ("due 2026-10-21"): the UI says "Wed Oct 21" (keepDates, dates.ts). Typed values
// (inputs), code, and imported pages (the owner's own text) keep theirs.
async function isoDates(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const out = new Set<string>();
    const walk = document.createTreeWalker(document.querySelector("main")!, NodeFilter.SHOW_TEXT);
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      const el = n.parentElement!;
      if (!el.getClientRects().length || el.closest("input, textarea, select, code, pre, .prose, .page, .sr-only, [aria-hidden=true]")) continue;
      if (/\d{4}-\d{2}-\d{2}/.test(n.textContent!)) out.add(`${el.tagName.toLowerCase()} "${n.textContent!.trim().slice(0, 40)}"`);
    }
    return [...out];
  });
}

async function sideways(page: Page): Promise<boolean> {
  return page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
}

for (const mode of MODES) {
  test(`all pages meet the comfort budget (${mode.name})`, async ({ screen }) => {
    const page = await screen({ viewport: { width: mode.width, height: mode.height }, colorScheme: mode.scheme, reducedMotion: "reduce", hasTouch: mode.touch });
    test.setTimeout(240_000);
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));
    page.on("console", (m) => m.type() === "error" && !TRACE_IN_SANDBOX.test(m.text()) && errors.push(m.text()));
    await page.goto(`/#token=${TOKEN}`);
    const dir = `${OUT}/comfort/${mode.name}`;
    mkdirSync(dir, { recursive: true });
    const found: Record<string, { axe: string[]; text: string[]; targets: string[]; spills: string[]; dates?: string[]; sideways: boolean }> = {};
    for (const p of PAGES) {
      await settle(page, p);
      // every folded part open, so its content is checked too
      await page.evaluate(() => document.querySelectorAll("details").forEach((d) => { d.open = true; }));
      const axe = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]).analyze();
      found[p.name] = {
        axe: axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical")
          .map((v) => `${v.id}: ${v.nodes.slice(0, 3).map((n) => n.target.join(" ")).join(" | ")}`),
        text: await smallText(page),
        targets: mode.touch ? await smallTargets(page) : [],
        spills: await spills(page),
        dates: await isoDates(page),
        sideways: await sideways(page),
      };
      await page.screenshot({ path: `${dir}/${p.name}.png`, fullPage: true });
    }
    // On a phone the views sit behind Menu: open it once and measure its links too.
    if (mode.touch) {
      await page.getByRole("button", { name: "Menu" }).click();
      await expect(page.getByRole("navigation", { name: "Views" })).toBeVisible();
      found.menu = { axe: [], text: await smallText(page), targets: await smallTargets(page), spills: await spills(page), sideways: await sideways(page) };
      await page.screenshot({ path: `${dir}/menu.png` });
      await page.getByRole("button", { name: "Menu" }).click();
    }
    writeFileSync(`${dir}/found.json`, JSON.stringify(found, null, 2));
    const problems = Object.entries(found).flatMap(([name, f]) => [
      ...f.axe.map((x) => `${name}: axe ${x}`),
      ...f.text.map((x) => `${name}: text under ${MIN_TEXT}px: ${x}`),
      ...f.targets.map((x) => `${name}: target under ${MIN_TARGET}px: ${x}`),
      ...f.spills.map((x) => `${name}: text spills: ${x}`),
      ...(f.dates ?? []).map((x) => `${name}: an ISO date in the UI: ${x}`),
      ...(f.sideways ? [`${name}: scrolls sideways`] : []),
    ]);
    expect(problems, `comfort budget problems (${mode.name}); details in ${dir}/found.json`).toEqual([]);
    expect(errors).toEqual([]);
  });
}

// The phone shell: one Menu button opens the grouped views, a view closes it again.
test("on a phone, Menu opens the views, and opening one closes the menu", async ({ screen }) => {
  const page = await screen({ viewport: { width: 375, height: 812 }, hasTouch: true });
  await page.goto(`/#token=${TOKEN}`);
  const menu = page.getByRole("button", { name: "Menu" });
  const nav = page.getByRole("navigation", { name: "Views" });
  await expect(menu).toHaveAttribute("aria-expanded", "false");
  await expect(nav).toBeHidden();
  await menu.click();
  await expect(menu).toHaveAttribute("aria-expanded", "true");
  await expect(nav.getByRole("link", { name: "Week", exact: true })).toBeVisible();
  await nav.getByRole("link", { name: "Week", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Week" })).toBeVisible();
  await expect(nav).toBeHidden();
  await expect(menu).toHaveAttribute("aria-expanded", "false");
  // the view you are on closes the menu too: the route stays, so only the click can close it
  await menu.click();
  await nav.getByRole("link", { name: "Week", exact: true }).click();
  await expect(nav).toBeHidden();
  await page.screenshot({ path: `${OUT}/phone-menu.png` });
});

// Taps per everyday job (the KLM-style budget from the comfort patterns): each job counts its clicks and asserts
// the budget. The count is the steps written here, so the budget pins the path: a control that moves behind a
// fold fails its locator, not the count. These jobs change no seed state, so later files see the same data.
test("everyday jobs fit their tap budget on a phone", async ({ screen }) => {
  const page = await screen({ viewport: { width: 375, height: 812 }, hasTouch: true, permissions: ["microphone", "clipboard-read", "clipboard-write"] });
  await page.goto(`/#token=${TOKEN}`);
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
  let taps = 0;
  const tap = async (l: ReturnType<Page["getByRole"]>) => { taps += 1; await l.click(); };
  const job = async (name: string, budget: number, run: () => Promise<void>) => {
    taps = 0;
    await run();
    expect(taps, `${name}: taps`).toBeLessThanOrEqual(budget);
  };
  await job("open Today from another view", 2, async () => {
    await tap(page.getByRole("button", { name: "Menu" }));
    await tap(page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Today", exact: true }));
    await expect(page.getByRole("heading", { level: 1, name: "Today" })).toBeVisible();
  });
  await job("copy the PM update", 1, async () => {
    await tap(page.getByRole("button", { name: "Copy update", exact: true }));
    await expect(page.getByText("Copied, paste it in Slack")).toBeVisible();
  });
  await page.evaluate(() => { window.location.hash = "#/practice"; });
  await job("see a flashcard's answer", 1, async () => {
    await tap(page.getByRole("button", { name: "Show answer (Space)" }));
    await expect(page.getByRole("button", { name: /Good/ })).toBeVisible();
  });
});

test("comfort budget leaves no test-created contexts", async ({ browser }) => {
  const count = browser.contexts().length;
  mkdirSync(OUT, { recursive: true });
  writeFileSync(`${OUT}/comfort-context-count.json`, JSON.stringify(count));
  expect(count).toBe(0);
});
