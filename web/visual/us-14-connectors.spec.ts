import { open, patch, region, shot, test } from "./helpers";

// US-14: the six work tools, each connected with a token kept in the Keychain or 1Password, never on disk.
const ready = (kind: string, secret: string) => ({ kind, origin: "user", secret, secret_ok: true, missing: [], ready: true, steps: [] });

test("US-14: six connected work tools", async ({ page }) => {
  await patch(page, "**/api/setup", (s) => ({ ...s, connectors: [ready("slack", "keychain"), ready("gdocs", "keychain"), ready("jira", "1password"), ready("github", "keychain"), ready("zoom", "keychain"), ready("outlook", "keychain")] }));
  await open(page, "setup");
  await shot(region(page, "Connectors"), "connectors");
});
