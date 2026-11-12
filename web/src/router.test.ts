import { describe, expect, it } from "vitest";
import { isBlocked, navGroups, parseHash, parseState, VIEWS, withState } from "./router";

describe("parseHash", () => {
  it("maps every view id and falls back to the dashboard", () => {
    for (const v of VIEWS) expect(parseHash(`#/${v.id}`).view).toBe(v.id);
    expect(parseHash("").view).toBe("dashboard");
    expect(parseHash("#/nonsense").view).toBe("dashboard");
  });
  it("reads a decoded parameter for company pages", () => {
    expect(parseHash("#/interviews/Example%20Co")).toEqual({ view: "interviews", param: "Example Co" });
  });
  it("reads the route before the view's state (issue #146)", () => {
    expect(parseHash("#/today?suggestions=2")).toEqual({ view: "today", param: undefined });
    expect(parseHash("#/notifications/n-1?open=2")).toEqual({ view: "notifications", param: "n-1" });
    expect(parseHash("#/?stale-sources=2").view).toBe("dashboard");
  });
});

describe("the view's state in the hash (issue #146)", () => {
  it("keeps bare tokens only, so typed text never lands in the URL", () => {
    expect(parseState("#/today?suggestions=2&suggestions-filter=agent-task")).toEqual({ suggestions: "2", "suggestions-filter": "agent-task" });
    expect(parseState("#/today")).toEqual({});
    // Upper case, spaces, encoded text, a second "=", an empty value, and a key ending in "token" are all dropped.
    expect(parseState("#/today?q=Example%20Co&Big=1&a=b=c&e=&mytoken=x&ok=1")).toEqual({ ok: "1" });
  });

  it("writes sorted keys after the route, drops nulls and anything not bare, and keeps the route", () => {
    expect(withState("#/today", { suggestions: "2" })).toBe("#/today?suggestions=2");
    expect(withState("#/today?suggestions=2", { "open-asks": "3", suggestions: null })).toBe("#/today?open-asks=3");
    expect(withState("#/today?suggestions=2", { suggestions: null })).toBe("#/today");
    expect(withState("#/interviews/Example%20Co", { companies: "2" })).toBe("#/interviews/Example%20Co?companies=2");
    expect(withState("#/library", { filter: "my private note" })).toBe("#/library");
    expect(withState("#/today", { token: "abc", "x-token": "abc" })).toBe("#/today");
    expect(withState("", { days: "14" })).toBe("#/?days=14");
  });

  it("a written state reads back the same", () => {
    const hash = withState("#/week", { days: "14", rows: "3" });
    expect(parseState(hash)).toEqual({ days: "14", rows: "3" });
    expect(parseHash(hash).view).toBe("week");
  });
});

describe("navGroups (issue #65)", () => {
  const names = (role: string | null | undefined) => navGroups(VIEWS, role).map((g) => `${g.group}: ${g.views.map((v) => v.id).join(",")}`);

  it("hides a group when every view in it is blocked", () => {
    expect(VIEWS.filter((v) => isBlocked(v, "corporate")).map((v) => v.id)).toEqual(["jobs", "interviews", "practice", "library"]);
    expect(names("corporate")).toEqual(["Plan: dashboard,teach,today,standup,notifications,goals,week,work", "System: review,reports,actions,setup"]);
  });

  it("keeps a group with at least one allowed view, without its blocked ones", () => {
    const mixed = [{ id: "a", group: "G", personalOnly: true }, { id: "b", group: "G" }, { id: "c", group: "H", personalOnly: true }];
    expect(navGroups(mixed, "corporate")).toEqual([{ group: "G", views: [{ id: "b", group: "G" }] }]);
  });

  it("shows every view on a personal machine or when the role could not load", () => {
    const all = ["Plan", "Job search", "System"];
    expect(navGroups(VIEWS, "personal").map((g) => g.group)).toEqual(all);
    expect(navGroups(VIEWS, null).map((g) => g.group)).toEqual(all);
  });

  it("shows nothing until the role is known, so no blocked link flashes and the sidebar never shifts", () => {
    expect(navGroups(VIEWS, undefined)).toEqual([]);
  });
});

describe("sidebar groups (issue #66)", () => {
  it("puts Work in Plan and Review in System, so no group mixes your career with careerbrew itself", () => {
    expect(navGroups(VIEWS, "personal").map((g) => `${g.group}: ${g.views.map((v) => v.id).join(",")}`)).toEqual([
      "Plan: dashboard,teach,today,standup,notifications,goals,week,work",
      "Job search: jobs,interviews,practice,library",
      "System: review,reports,actions,setup",
    ]);
  });

  it("says what Actions is for in the first line under its title", () => {
    expect(VIEWS.find((v) => v.id === "actions")!.hint).toMatch(/^Run any careerbrew command from the browser/);
  });
});
