import { afterEach, expect, it } from "vitest";
import { localDate, localStamp } from "./api";

// US-75 visual review: an agent's approval request said "Requested 08:45" for a request made at 04:45 at UTC-4:
// the app cut the UTC stamp as if it were local time. Stamps with an offset show in this browser's time zone.
const zone = process.env.TZ;
afterEach(() => {
  // this worker process's other test files keep their zone
  if (zone === undefined) delete process.env.TZ;
  else process.env.TZ = zone;
});

it("shows a stamp with an offset in the browser's time zone, and a local one as written", () => {
  process.env.TZ = "Etc/GMT+4";
  expect(localStamp("2026-10-19T08:45:00+00:00")).toBe("2026-10-19 04:45");
  expect(localStamp("2026-10-20T01:30:00Z")).toBe("2026-10-19 21:30");
  expect(localStamp("2026-10-19T09:00:00-04:00")).toBe("2026-10-19 09:00");
  expect(localStamp("2026-10-19T09:00")).toBe("2026-10-19 09:00");
  expect(localDate("2026-10-20T01:30:00Z")).toBe("2026-10-19");
  expect(localStamp("not a date")).toBe("not a date");
});
