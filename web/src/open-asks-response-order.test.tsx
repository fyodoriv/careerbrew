import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { clearCache } from "./cache";
import { CHANGED_EVENT } from "./live";
import { useData } from "./useData";

afterEach(() => { cleanup(); clearCache(); vi.unstubAllGlobals(); });

for (const order of ["default-first", "expanded-first"] as const) {
  it(`keeps the expanded result after a live refresh: ${order}`, async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("synthetic tab key unavailable")));
    let defaultReads = 0;
    let releaseDefault: ((value: number) => void) | undefined;
    let releaseExpanded: ((value: number) => void) | undefined;
    const load = vi.fn((expanded: boolean) => {
      if (expanded) return new Promise<number>((resolve) => { releaseExpanded = resolve; });
      defaultReads += 1;
      if (defaultReads === 1) return Promise.resolve(50);
      return new Promise<number>((resolve) => { releaseDefault = resolve; });
    });
    function Probe({ expanded }: { expanded: boolean }) {
      const q = useData(() => load(expanded), [expanded], { live: true, key: expanded ? "/api/feed/asks?limit=60" : "/api/feed/asks" });
      return <p>asks={q.data ?? "-"}</p>;
    }
    const view = render(<Probe expanded={false} />);
    expect(await screen.findByText("asks=50")).toBeInTheDocument();
    await act(async () => { window.dispatchEvent(new Event(CHANGED_EVENT)); });
    expect(releaseDefault).toBeDefined();
    view.rerender(<Probe expanded />);
    expect(releaseExpanded).toBeDefined();
    if (order === "default-first") {
      await act(async () => { releaseDefault?.(50); });
      await act(async () => { releaseExpanded?.(60); });
    } else {
      await act(async () => { releaseExpanded?.(60); });
      expect(screen.getByText("asks=60")).toBeInTheDocument();
      await act(async () => { releaseDefault?.(50); });
    }
    expect(screen.getByText("asks=60")).toBeInTheDocument();
    expect(load.mock.calls.map(([expanded]) => expanded)).toEqual([false, false, true]);
  });
}
