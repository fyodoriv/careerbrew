import type { ReactNode } from "react";
import { About } from "./about";
import { facetCounts, facetOf, pageSlice, usePaging } from "./paging";

// A filter shown as a select (issue #147): one option per value, with its count. `name` gives a value its words.
export interface Choice<T> { key: string; label: string; of: (row: T) => string; name?: (value: string) => string }
// An order for the rows. The first one is the list's own order (most important first) and never goes in the hash.
export interface Sort<T> { key: string; label: string; cmp: (a: T, b: T) => number }

// Issue #146: one pager for every list of live data. A list shows one page of rows, "Showing 1–10 of 48", and
// Previous and Next; an optional filter shows one button per value with its count. The page and the filter live in
// the hash (`#/today?suggestions=2&suggestions-filter=goal`), so a reload keeps the place. Sizes: PAGE_SIZE in order.ts.
// Issue #147: more filters as selects (`choices`, hash `<id>-<key>`) and a sort (`sorts`, hash `<id>-sort`). Each
// filter counts the rows the other filters keep, so its counts always add up to what it would show.
export function Paged<T>({ id, rows, size, noun, facet, choices, sorts, focus, children }: {
  id: string;  // the hash key: a bare token, unique on its view
  rows: readonly T[];  // most important first
  size: number;
  noun: string;  // what the rows are, plural ("suggestions"): names the pager and the filter
  facet?: (row: T) => string;  // filter the rows by this value (a kind): one button per value
  choices?: Choice<T>[];  // more filters, each a select
  sorts?: Sort<T>[];  // orders to choose from; the first is the rows' own order
  focus?: (row: T) => boolean;  // with no page in the hash, show the page of the first row this picks
  children: (shown: T[]) => ReactNode;
}) {
  const paging = usePaging(id);
  const filters = [...(facet ? [{ key: "filter", of: facet }] : []), ...(choices ?? [])];
  const chosen = filters.map((f) => {
    const v = f.key === "filter" ? paging.filter : paging.option(f.key);
    return v && facetCounts(rows, f.of).some(([x]) => x === v) ? v : null;
  });
  // The rows every chosen filter keeps, leaving out filter `skip` (its own counts ignore it).
  const keeps = (r: T, skip: number) => filters.every((f, i) => i === skip || !chosen[i] || facetOf(f.of, r) === chosen[i]);
  const sort = sorts?.slice(1).find((x) => x.key === paging.option("sort")) ?? null;
  const kept = rows.filter((r) => keeps(r, -1));
  const ordered = sort ? [...kept].sort(sort.cmp) : kept;
  const narrowed = chosen.some(Boolean);
  const tools = rows.length > size || narrowed || sort !== null;  // tools help only a list longer than one page
  const counts = (i: number) => {
    const base = rows.filter((r) => keeps(r, i));
    const values = facetCounts(base, filters[i].of);
    if (chosen[i] && !values.some(([v]) => v === chosen[i])) values.push([chosen[i] as string, 0]);
    return { base: base.length, values };
  };
  const shows = (i: number) => tools && facetCounts(rows, filters[i].of).length > 1;
  const focused = paging.page === null && focus ? ordered.findIndex(focus) : -1;
  const s = pageSlice(ordered, paging.page ?? (focused >= 0 ? Math.floor(focused / size) + 1 : 1), size);
  const kinds = facet && shows(0) ? counts(0) : null;
  const offset = facet ? 1 : 0;
  return (
    <>
      {/* A filter helps only a list longer than one page; a chosen one always shows, so All can clear it. */}
      {kinds && (
        <div className="chips pager-filter" role="group" aria-label={`Filter ${noun}`}>
          <button type="button" className="chip" aria-pressed={!chosen[0]} onClick={() => paging.setFilter(null)}>All ({kinds.base})</button>
          {kinds.values.map(([v, n]) => (
            <button key={v} type="button" className="chip" aria-pressed={chosen[0] === v} onClick={() => paging.setFilter(v)}>{v} ({n})</button>
          ))}
        </div>
      )}
      {(choices?.some((_, j) => shows(offset + j)) || (tools && sorts && sorts.length > 1)) && (
        <div className="pager-tools">
          {choices?.map((c, j) => {
            if (!shows(offset + j)) return null;
            const { base, values } = counts(offset + j);
            return (
              <label key={c.key} className="field">{c.label}
                <select aria-label={`Filter ${noun} by ${c.label.toLowerCase()}`} value={chosen[offset + j] ?? ""} onChange={(e) => paging.setOption(c.key, e.target.value || null)}>
                  <option value="">All ({base})</option>
                  {values.map(([v, n]) => <option key={v} value={v}>{c.name ? c.name(v) : v} ({n})</option>)}
                </select>
              </label>
            );
          })}
          {tools && sorts && sorts.length > 1 && (
            <label className="field">Sort
              <select aria-label={`Sort ${noun}`} value={sort?.key ?? sorts[0].key} onChange={(e) => paging.setOption("sort", e.target.value === sorts[0].key ? null : e.target.value)}>
                {sorts.map((x) => <option key={x.key} value={x.key}>{x.label}</option>)}
              </select>
            </label>
          )}
        </div>
      )}
      {children(s.rows)}
      {s.total > size && (
        <div className="pager" role="group" aria-label={`Pages of ${noun}`} data-pager={id}>
          <span className="muted small">Showing {s.from}–{s.to} of {s.total}</span>
          <span className="buttons">
            <button type="button" className="secondary" disabled={s.page <= 1} onClick={() => paging.setPage(s.page - 1)}>Previous</button>
            <button type="button" className="secondary" disabled={s.page >= s.pages} onClick={() => paging.setPage(s.page + 1)}>Next</button>
          </span>
        </div>
      )}
    </>
  );
}

// A block that can fold away; its header always shows its count ("Closed (12)"), folded or not. `open` is where it
// starts; the owner's toggle then holds until the page reloads.
export function Fold({ title, count, open, block, children }: { title: string; count: number; open?: boolean; block?: string; children: ReactNode }) {
  const label = `${title} (${count})`;
  return (
    <section className="card fold" aria-label={label} data-block={block}>
      {block && <About block={block} />}
      <details open={open}>
        <summary><h2>{label}</h2></summary>
        {children}
      </details>
    </section>
  );
}
