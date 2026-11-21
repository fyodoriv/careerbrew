import { describe, expect, it } from "vitest";
import { isHealthRead } from "../e2e/comfort-health-boundary";

describe("PM geometry health request boundary", () => {
  const origin = "http://127.0.0.1:43210";

  it("allows the exact same-origin health read", () => {
    expect(isHealthRead("GET", new URL(`${origin}/api/feed/health`), origin)).toBe(true);
  });

  it.each([
    ["POST", `${origin}/api/feed/health`],
    ["GET", "http://127.0.0.1:43211/api/feed/health"],
    ["GET", `${origin}/api/feed/health?extra=1`],
    ["GET", `${origin}/api/feed/health/other`],
  ])("refuses %s %s", (method, address) => {
    expect(isHealthRead(method, new URL(address), origin)).toBe(false);
  });
});
