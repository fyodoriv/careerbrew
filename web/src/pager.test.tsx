import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Fold, Paged } from "./pager";
import { facetCounts, pageSlice, slug } from "./paging";

// Issue #146: one pager for every list of live data. Its page and filter live in the URL hash.

const nums = (n: number) => Array.from({ length: n }, (_, i) => i + 1);

describe("pageSlice", () => {
  it("cuts one page and says which rows it shows", () => {
    expect(pageSlice(nums(48), 1, 10)).toEqual({ rows: nums(10), page: 1, pages: 5, from: 1, to: 10, total: 48 });
    expect(pageSlice(nums(48), 5, 10)).toMatchObject({ rows: [41, 42, 43, 44, 45, 46, 47, 48], from: 41, to: 48 });
  });

  it("shows the last page past the end and the first below 1, so a shrunk list never shows an empty page", () => {
    expect(pageSlice(nums(12), 9, 10)).toMatchObject({ page: 2, rows: [11, 12] });
    expect(pageSlice(nums(12), 0, 10)).toMatchObject({ page: 1, from: 1 });
    expect(pageSlice(nums(12), Number.NaN, 10)).toMatchObject({ page: 1 });
    expect(pageSlice([], 3, 10)).toEqual({ rows: [], page: 1, pages: 1, from: 0, to: 0, total: 0 });
  });
});

describe("hash keys from data", () => {
  it("slug makes a bare token of a group name", () => {
    expect(slug("Other work")).toBe("other-work");
    expect(slug("Shipped")).toBe("shipped");
    expect(slug("Équipe / Ops!")).toBe("quipe-ops");
    expect(slug("???")).toBe("other");
  });

  it("facetCounts counts each filter value, the biggest first; a value that is not a bare token counts as other", () => {
    const kinds = ["goal", "agent-task", "agent-task", "New Task", "x y"];
    expect(facetCounts(kinds, (k) => k)).toEqual([["agent-task", 2], ["other", 2], ["goal", 1]]);
  });
});

function List({ n, size = 10, focus }: { n: number; size?: number; focus?: number }) {
  return (
    <Paged id="items" rows={nums(n)} size={size} noun="items" focus={focus === undefined ? undefined : (r) => r === focus}>
      {(rows) => <ul aria-label="Items">{rows.map((r) => <li key={r}>item {r}</li>)}</ul>}
    </Paged>
  );
}

const shown = () => within(screen.getByRole("list", { name: "Items" })).getAllByRole("listitem").map((li) => li.textContent);

describe("Paged", () => {
  beforeEach(() => window.history.replaceState(null, "", "#/today"));
  afterEach(cleanup);

  it("shows every row and no pager when they fit on one page", () => {
    render(<List n={10} />);
    expect(shown()).toHaveLength(10);
    expect(screen.queryByRole("group", { name: "Pages of items" })).toBeNull();
    expect(screen.queryByText(/^Showing/)).toBeNull();
  });

  it("shows one page, 'Showing 1–10 of 48', and Next and Previous that keep the page in the hash", () => {
    render(<List n={48} />);
    const pager = screen.getByRole("group", { name: "Pages of items" });
    expect(shown()).toEqual(nums(10).map((i) => `item ${i}`));
    expect(pager).toHaveTextContent("Showing 1–10 of 48");
    expect(within(pager).getByRole("button", { name: "Previous" })).toBeDisabled();
    fireEvent.click(within(pager).getByRole("button", { name: "Next" }));
    expect(shown()[0]).toBe("item 11");
    expect(pager).toHaveTextContent("Showing 11–20 of 48");
    expect(window.location.hash).toBe("#/today?items=2");
    fireEvent.click(within(pager).getByRole("button", { name: "Previous" }));
    expect(window.location.hash).toBe("#/today");  // page 1 is the default: no key
  });

  it("opens on the page the hash names, as after a reload, and disables Next on the last page", () => {
    window.history.replaceState(null, "", "#/today?items=5");
    render(<List n={48} />);
    expect(shown()).toEqual([41, 42, 43, 44, 45, 46, 47, 48].map((i) => `item ${i}`));
    expect(screen.getByText("Showing 41–48 of 48")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
  });

  it("follows a hash changed by hand or by Back", () => {
    render(<List n={48} />);
    act(() => {
      window.history.replaceState(null, "", "#/today?items=3");
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    expect(shown()[0]).toBe("item 21");
  });

  it("with no page in the hash, opens the page of the focused row", () => {
    render(<List n={48} focus={27} />);
    expect(shown()).toContain("item 27");
    expect(screen.getByText("Showing 21–30 of 48")).toBeInTheDocument();
  });

  it("filters by a value with one button per value and its count, keeps the filter in the hash, and starts at page 1", () => {
    window.history.replaceState(null, "", "#/today?kinds=2");
    const rows = [...nums(23).map((i) => ({ id: i, kind: "agent-task" })), ...nums(4).map((i) => ({ id: 100 + i, kind: "goal" }))];
    render(
      <Paged id="kinds" rows={rows} size={10} noun="suggestions" facet={(r) => r.kind}>
        {(page) => <ul aria-label="Items">{page.map((r) => <li key={r.id}>{r.kind} {r.id}</li>)}</ul>}
      </Paged>,
    );
    const filter = screen.getByRole("group", { name: "Filter suggestions" });
    expect(within(filter).getAllByRole("button").map((b) => b.textContent)).toEqual(["All (27)", "agent-task (23)", "goal (4)"]);
    expect(within(filter).getByRole("button", { name: "All (27)" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(within(filter).getByRole("button", { name: "goal (4)" }));
    expect(window.location.hash).toBe("#/today?kinds-filter=goal");
    expect(shown()).toEqual(["goal 101", "goal 102", "goal 103", "goal 104"]);
    expect(screen.queryByRole("group", { name: "Pages of suggestions" })).toBeNull();
    fireEvent.click(within(filter).getByRole("button", { name: "All (27)" }));
    expect(window.location.hash).toBe("#/today");
  });

  it("shows no filter for a list that fits on one page, unless a filter is chosen, so All can clear it", () => {
    const rows = [{ k: "a" }, { k: "b" }, { k: "b" }];
    const list = <Paged id="kinds" rows={rows} size={10} noun="rows" facet={(r) => r.k}>{(page) => <ul aria-label="Items">{page.map((r, i) => <li key={i}>{r.k}</li>)}</ul>}</Paged>;
    const { unmount } = render(list);
    expect(screen.queryByRole("group", { name: "Filter rows" })).toBeNull();
    unmount();
    window.history.replaceState(null, "", "#/today?kinds-filter=b");
    render(list);
    expect(shown()).toEqual(["b", "b"]);
    fireEvent.click(within(screen.getByRole("group", { name: "Filter rows" })).getByRole("button", { name: "All (3)" }));
    expect(shown()).toEqual(["a", "b", "b"]);
  });

  it("ignores a filter in the hash that no row has, so a stale link still shows the list", () => {
    window.history.replaceState(null, "", "#/today?kinds-filter=gone");
    render(
      <Paged id="kinds" rows={[{ k: "a" }, { k: "b" }]} size={10} noun="rows" facet={(r) => r.k}>
        {(page) => <ul aria-label="Items">{page.map((r) => <li key={r.k}>{r.k}</li>)}</ul>}
      </Paged>,
    );
    expect(shown()).toEqual(["a", "b"]);
  });
});

describe("Paged: filters as selects and a sort (issue #147)", () => {
  beforeEach(() => window.history.replaceState(null, "", "#/today"));
  afterEach(cleanup);
  type Row = { id: number; kind: string; project: string; at: string };
  const rows: Row[] = nums(14).map((i) => ({ id: i, kind: i % 2 ? "source" : "agent-task", project: i <= 4 ? "syn-billing" : "", at: `2026-10-${String(i).padStart(2, "0")}` }));
  const list = () => (
    <Paged
      id="sugs" rows={rows} size={10} noun="suggestions" facet={(r) => r.kind}
      choices={[{ key: "project", label: "Project", of: (r) => r.project || "none", name: (v) => (v === "none" ? "no project" : v) }]}
      sorts={[{ key: "oldest", label: "Oldest first", cmp: () => 0 }, { key: "newest", label: "Newest first", cmp: (a, b) => b.at.localeCompare(a.at) }]}
    >
      {(page) => <ul aria-label="Items">{page.map((r) => <li key={r.id}>{r.id}</li>)}</ul>}
    </Paged>
  );

  it("filters by project with each value's count, and the kind buttons count the rows the project keeps", () => {
    render(list());
    const project = screen.getByRole("combobox", { name: "Filter suggestions by project" });
    expect(within(project).getAllByRole("option").map((o) => o.textContent)).toEqual(["All (14)", "no project (10)", "syn-billing (4)"]);
    fireEvent.change(project, { target: { value: "syn-billing" } });
    expect(window.location.hash).toBe("#/today?sugs-project=syn-billing");
    expect(shown()).toEqual(["1", "2", "3", "4"]);
    const kinds = screen.getByRole("group", { name: "Filter suggestions" });
    expect(within(kinds).getAllByRole("button").map((b) => b.textContent)).toEqual(["All (4)", "agent-task (2)", "source (2)"]);
    fireEvent.click(within(kinds).getByRole("button", { name: "source (2)" }));
    expect(shown()).toEqual(["1", "3"]);
    expect(within(screen.getByRole("combobox", { name: "Filter suggestions by project" })).getAllByRole("option").map((o) => o.textContent)).toEqual(["All (7)", "no project (5)", "syn-billing (2)"]);
  });

  it("sorts, keeps the sort in the hash, starts at page 1, and the first order writes nothing", () => {
    window.history.replaceState(null, "", "#/today?sugs=2");
    render(list());
    const sort = screen.getByRole("combobox", { name: "Sort suggestions" });
    fireEvent.change(sort, { target: { value: "newest" } });
    expect(window.location.hash).toBe("#/today?sugs-sort=newest");
    expect(shown().slice(0, 3)).toEqual(["14", "13", "12"]);
    fireEvent.change(sort, { target: { value: "oldest" } });
    expect(window.location.hash).toBe("#/today");
    expect(shown()[0]).toBe("1");
  });

  it("shows no select for a list that fits on one page", () => {
    render(<Paged id="sugs" rows={rows.slice(0, 3)} size={10} noun="suggestions" choices={[{ key: "project", label: "Project", of: (r) => r.project || "none" }]} sorts={[{ key: "a", label: "A", cmp: () => 0 }, { key: "b", label: "B", cmp: () => 0 }]}>{(page) => <ul aria-label="Items">{page.map((r) => <li key={r.id}>{r.id}</li>)}</ul>}</Paged>);
    expect(screen.queryByRole("combobox")).toBeNull();
  });
});

describe("Fold", () => {
  afterEach(cleanup);

  it("shows its count in its header whether it is folded or open", () => {
    render(<Fold title="Closed" count={12} block="closed"><p>a closed row</p></Fold>);
    const region = screen.getByRole("region", { name: "Closed (12)" });
    expect(region).toHaveAttribute("data-block", "closed");
    expect(within(region).getByRole("heading", { name: "Closed (12)" })).toBeInTheDocument();
    expect(region.querySelector("details")!.open).toBe(false);
    cleanup();
    render(<Fold title="Closed" count={12} open><p>a closed row</p></Fold>);
    expect(screen.getByRole("region", { name: "Closed (12)" }).querySelector("details")!.open).toBe(true);
  });
});
