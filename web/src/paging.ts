import { isBare, useHashState } from "./router";

// Issue #146: the page math and the hash state behind `Paged` (pager.tsx).

export interface PageSlice<T> { rows: T[]; page: number; pages: number; from: number; to: number; total: number }

// One page of `rows`; pages count from 1. A page past the end shows the last page, and anything below 1 the first,
// so a list that shrank (rows accepted elsewhere) never shows an empty page.
export function pageSlice<T>(rows: readonly T[], page: number, size: number): PageSlice<T> {
  const total = rows.length;
  const pages = Math.max(1, Math.ceil(total / size));
  const p = Math.min(Math.max(1, Math.floor(page) || 1), pages);
  const start = (p - 1) * size;
  const shown = rows.slice(start, start + size);
  return { rows: shown, page: p, pages, from: total ? start + 1 : 0, to: start + shown.length, total };
}

// A hash key part from a name the data chose (a group label such as "Other work"): a bare token.
export function slug(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 24) || "other";
}

// A filter value as a bare token: the hash keeps nothing else, so any other value counts as "other".
export function facetOf<T>(facet: (row: T) => string, row: T): string {
  const v = facet(row);
  return isBare(v) ? v : "other";
}

// Each filter value with its count, the biggest first.
export function facetCounts<T>(rows: readonly T[], facet: (row: T) => string): [string, number][] {
  const counts = new Map<string, number>();
  for (const r of rows) {
    const v = facetOf(facet, r);
    counts.set(v, (counts.get(v) ?? 0) + 1);
  }
  return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

// A list's page (null when the hash names none), its filter, and its other options (`<id>-<key>`: a project, a
// priority, a sort; issue #147) in the hash, with setters. A new filter or option starts at page 1.
export function usePaging(id: string) {
  const [state, set] = useHashState();
  const n = Number(state[id]);
  return {
    page: Number.isInteger(n) && n >= 1 ? n : null,
    filter: state[`${id}-filter`] ?? null,
    setPage: (p: number) => set({ [id]: p > 1 ? String(p) : null }),
    setFilter: (f: string | null) => set({ [`${id}-filter`]: f, [id]: null }),
    option: (key: string): string | null => state[`${id}-${key}`] ?? null,
    setOption: (key: string, v: string | null) => set({ [`${id}-${key}`]: v, [id]: null }),
  };
}
