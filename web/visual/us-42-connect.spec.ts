import { open, patch, region, shot, test } from "./helpers";

// US-42: a connector that is not working lists the steps that finish it.
test("US-42: a connector that needs setup", async ({ page }) => {
  await patch<{ connectors: object[] }>(page, "**/api/setup", (s) => ({
    ...s, connectors: [...s.connectors, { kind: "jira", origin: "user", secret: null, secret_ok: false, missing: ["email", "token"], ready: false,
      steps: ["Make an API token at https://id.atlassian.com/manage-profile/security/api-tokens", "careerbrew connect jira --email you@example.com", "careerbrew secret set jira (paste the token when asked)"] }],
  }));
  await open(page, "setup");
  await shot(region(page, "Connectors"), "connector-needs-setup");
});
