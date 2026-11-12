import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { ActStatus } from "./act-status";

// US-75 visual review: a failed action's alert showed the fix with raw backticks ("run `scripts/doctor.sh`"),
// while a failed block showed the same fix with each command as code. An action error now reads like a block error.
it("shows the commands in an action's error as code, without backticks", () => {
  render(<ActStatus act={{ busy: false, error: "synthetic failure. Fix: check the terminal that runs `careerbrew serve`, run `scripts/doctor.sh`, then try again." }} />);
  const alert = screen.getByRole("alert");
  expect(alert).not.toHaveTextContent("`");
  expect([...alert.querySelectorAll("code")].map((c) => c.textContent)).toEqual(["careerbrew serve", "scripts/doctor.sh"]);
});
