import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

// The views.test flake (2026-10-25): a view an earlier test rendered stayed mounted and kept fetching.
describe("test setup", () => {
  it("renders a view", () => {
    render(<p>left over</p>);
    expect(screen.getByText("left over")).toBeInTheDocument();
  });

  it("starts the next test with nothing mounted", () => {
    expect(document.body.textContent).toBe("");
  });
});
