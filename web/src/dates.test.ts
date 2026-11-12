import { describe, expect, it } from "vitest";
import { friendlyDay, fullDay, longDay, shortDay } from "./dates";

describe("friendly day labels", () => {
  it("reads an ISO day as that calendar day (formatted in UTC, so the local zone never shifts it)", () => {
    expect(shortDay("2026-10-19")).toBe("Mon Oct 19");
    expect(longDay("2026-10-19")).toBe("Monday, Oct 19");
    expect(shortDay("2026-03-01")).toBe("Sun Mar 1");
    expect(fullDay("2027-01-17")).toBe("Sun Jan 17, 2027");
  });
  it("leaves anything that is not a day as it is", () => {
    expect(shortDay("tomorrow")).toBe("tomorrow");
    expect(longDay("")).toBe("");
    expect(friendlyDay("2026-13-45")).toBe("2026-13-45");
  });
});
