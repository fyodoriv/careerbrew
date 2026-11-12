import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../..", import.meta.url));
const seed = `
from careerbrew import config, server

config.set_value("pm_slack", "#e2e-disconnected")
with server.make_server(0, "e2e-token-not-secret") as http:
    print(f"pm-slack-fixture:{http.server_address[1]}", flush=True)
    http.serve_forever()
`;

export async function disconnectedSlackServer(now: string) {
  const root = mkdtempSync(join(tmpdir(), "careerbrew-pm-slack-"));
  const home = join(root, "home");
  mkdirSync(home);
  const child = spawn(join(repo, ".venv/bin/python"), ["-I", "-B", "-c", seed], {
    cwd: repo,
    env: {
      PATH: "/usr/bin:/bin",
      HOME: home,
      XDG_CONFIG_HOME: join(home, "config"),
      XDG_CACHE_HOME: join(home, "cache"),
      XDG_STATE_HOME: join(home, "state"),
      CAREERBREW_HOME: join(root, "data"),
      CAREERBREW_KEY_FILE: join(root, "key"),
      CAREERBREW_PAIR_KEY_FILE: join(root, "pair"),
      CAREERBREW_PUBLIC_CARDS: join(root, "cards"),
      CAREERBREW_BACKUP_DIRS: join(root, "backups"),
      CAREERBREW_COMMANDS_DIR: join(root, "commands"),
      CAREERBREW_CLAUDE_CONFIG: join(home, "claude.json"),
      CAREERBREW_CODEX_CONFIG: join(home, "codex.toml"),
      CAREERBREW_NOW: now,
      CAREERBREW_HTTP_FIXTURES: join(repo, "tests/fixtures/http"),
      CAREERBREW_LLM: "off",
      CAREERBREW_NOTIFY: "off",
      CAREERBREW_EVENTS: "off",
      CAREERBREW_POSTHOG_KEY: "",
      CAREERBREW_TELEMETRY_URL: "http://127.0.0.1:9",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let closed = false;
  const closing = new Promise<void>((resolve) => {
    child.once("close", () => { closed = true; resolve(); });
  });
  child.stderr.resume();
  let stopping: Promise<void> | undefined;
  const waitForClose = (ms: number) => new Promise<boolean>((resolve) => {
    const timer = setTimeout(() => resolve(false), ms);
    void closing.then(() => { clearTimeout(timer); resolve(true); });
  });
  const stop = () => {
    stopping ??= (async () => {
      if (!closed && child.pid !== undefined && child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
      if (!await waitForClose(2_000)) {
        if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
        if (!await waitForClose(2_000)) throw new Error("Disconnected Slack fixture did not close");
      }
      rmSync(root, { recursive: true, force: true });
    })();
    return stopping;
  };
  try {
    const port = await new Promise<number>((resolve, reject) => {
      let line = "";
      const timer = setTimeout(() => reject(new Error("Disconnected Slack fixture readiness timed out")), 10_000);
      child.once("error", () => { clearTimeout(timer); reject(new Error("Could not start disconnected Slack fixture")); });
      child.once("close", () => { clearTimeout(timer); reject(new Error("Disconnected Slack fixture closed before readiness")); });
      child.stdout.on("data", (data: Buffer) => {
        line += data.toString();
        const match = /^pm-slack-fixture:([1-9][0-9]{0,4})$/m.exec(line);
        if (!line.includes("\n") || !match || Number(match[1]) > 65535) return;
        clearTimeout(timer);
        resolve(Number(match[1]));
      });
    });
    return { origin: `http://127.0.0.1:${port}`, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}
