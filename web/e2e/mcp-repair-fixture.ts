import { spawn, spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, realpathSync, rmSync, statSync, symlinkSync, writeFileSync, writeSync } from "node:fs";
import { type AddressInfo, createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));
export const REPAIR_CANARY = "synthetic-header-canary-7";
export const REPAIR_TOKEN = "e2e-token-not-secret";

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const socket = createServer();
    socket.once("error", reject);
    socket.listen(0, "127.0.0.1", () => {
      const { port } = socket.address() as AddressInfo;
      socket.close((error) => error ? reject(error) : resolve(port));
    });
  });
}

// Each test owns one minimal server process group and scratch client. Browser HOME is never changed.
// stop() ends every process in that group before deleting only its own fixture directory.
export async function repairServer(repo = ROOT, now = process.env.CAREERBREW_NOW) {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), "careerbrew-mcp-browser-")));
  const client = join(dir, "client-home"), bin = join(dir, "bin");
  mkdirSync(client); mkdirSync(bin);
  const target = join(client, "owned-settings.json"), link = join(client, ".claude.json");
  const ghCalls = join(bin, "gh-calls");
  const original = JSON.stringify({ keep: { fixture: true }, mcpServers: { fixture: { type: "http", url: "https://api.githubcopilot.com/mcp", headers: { Authorization: "Bearer ${GITHUB_TOKEN}", "X-Test": "keep" } } } });
  const reset = () => { writeFileSync(target, original); chmodSync(target, 0o640); };
  reset(); symlinkSync(target, link);
  writeFileSync(join(bin, "launchctl"), "#!/bin/sh\nexit 0\n");
  writeFileSync(join(bin, "gh"), `#!/bin/sh\nd="$(dirname "$0")"\nprintf 'invoked\\n' >> "$d/gh-calls"\nprintf '%s\\n' '${REPAIR_CANARY}'\n`);
  writeFileSync(join(bin, "agentbrew"), '#!/bin/sh\n[ "$*" = "sync --only commands" ] || exit 2\n[ -f Agentfile.yaml ] || exit 3\n');
  for (const command of ["launchctl", "gh", "agentbrew"]) chmodSync(join(bin, command), 0o755);
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("CAREERBREW_") && !key.startsWith("CB_") && !["GH_TOKEN", "GITHUB_TOKEN", "PYTHONHOME", "PYTHONPATH"].includes(key)));
  Object.assign(env, {
    HOME: client, CAREERBREW_CLIENT_HOME: client, CAREERBREW_HOME: join(dir, "data"),
    CAREERBREW_CLAUDE_CONFIG: link, CAREERBREW_CODEX_CONFIG: join(client, ".codex/config.toml"), XDG_STATE_HOME: join(client, ".local/state"),
    CAREERBREW_KEY_FILE: join(dir, "key"), CAREERBREW_PAIR_KEY_FILE: join(dir, "pair"), CAREERBREW_BACKUP_DIRS: join(dir, "backups"),
    CAREERBREW_COMMANDS_DIR: join(dir, "commands"), CAREERBREW_SERVE_TOKEN: REPAIR_TOKEN,
    CAREERBREW_HTTP_FIXTURES: join(repo, "tests/fixtures/http"), CAREERBREW_EVENTS: "on", CAREERBREW_NOW: now,
    CAREERBREW_POSTHOG_KEY: "", CAREERBREW_TELEMETRY_URL: "http://127.0.0.1:9", CAREERBREW_LLM: "off", CAREERBREW_NOTIFY: "off",
    GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1", PATH: bin + ":" + process.env.PATH,
  });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const code = "import sys; from careerbrew import cli; assert cli.main(['init']) == 0; raise SystemExit(cli.main(['serve', '--port', sys.argv[1]]))";
  const child = spawn(join(repo, ".venv/bin/python"), ["-B", "-c", code, String(port)], { cwd: repo, env, detached: true, stdio: ["ignore", "pipe", "pipe"] });
  const exited = new Promise<void>((resolve) => {
    child.once("exit", () => resolve());
    child.once("error", () => resolve());
  });
  let output = "";
  let stopping: Promise<void> | undefined;
  const alive = () => {
    if (!child.pid) return false;
    try { process.kill(-child.pid, 0); return true; }
    catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ESRCH") return false;
      if ((error as NodeJS.ErrnoException).code !== "EPERM") throw error;
      // macOS can retain a group with only exited members before reaping finishes.
      const snapshot = spawnSync("ps", ["-axo", "pid=,pgid=,uid=,stat="], { encoding: "utf8", timeout: 5_000 });
      const rejectSnapshot = (reason: "ps-error" | "ps-nonzero" | "malformed-row" | "foreign-member" | "live-member"): never => {
        try {
          const lines = (snapshot.stdout ?? "").split("\n").filter((line) => line.trim());
          const projected: [number, number, number, string][] = [];
          let malformed = 0;
          for (const line of lines) {
            const row = line.match(/^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s*$/);
            const numbers = row?.slice(1, 4).map(Number);
            if (!row || !numbers || numbers.some((value) => !Number.isSafeInteger(value) || value < 0 || value > 4_294_967_295)) { malformed++; continue; }
            const state = /^[RSDZTIXWUNEH?][lLs+<>NVAESWX-]{0,12}$/.test(row[4]) ? row[4] : "unknown";
            projected.push([numbers[0], numbers[1], numbers[2], state]);
          }
          const members = projected.filter((row) => row[1] === child.pid);
          const code = (snapshot.error as NodeJS.ErrnoException | undefined)?.code;
          let worker: { node: string; executable: { device: number; inode: number } } | null = null;
          try {
            const node = process.versions.node;
            const executable = statSync(process.execPath);
            if (/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(node) &&
                Number.isSafeInteger(executable.dev) && executable.dev >= 0 &&
                Number.isSafeInteger(executable.ino) && executable.ino > 0) {
              worker = { node, executable: { device: executable.dev, inode: executable.ino } };
            }
          } catch { /* The original cleanup error stays authoritative. */ }
          const record = {
            worker,
            scope: "repair-fixture-invocation", reason, group: child.pid, expected_uid: process.getuid?.(),
            ps: {
              status: snapshot.status, signal: snapshot.signal && ["SIGTERM", "SIGKILL", "SIGABRT", "SIGSEGV", "SIGINT", "SIGHUP", "SIGPIPE", "SIGBUS", "SIGILL", "SIGTRAP"].includes(snapshot.signal) ? snapshot.signal : null,
              error_code: code && ["EPERM", "ESRCH", "ETIMEDOUT", "ENOBUFS", "ENOENT", "EACCES", "EAGAIN", "EINTR"].includes(code) ? code : null,
              total_rows: lines.length, malformed_rows: malformed,
              rows: projected.slice(0, 128), group_members: members.slice(0, 128), truncated: projected.length > 128 || members.length > 128,
            },
          };
          const message = "repair-fixture-cleanup: " + JSON.stringify(record) + "\n";
          if (Buffer.byteLength(message) <= 32_768) writeSync(2, message);
        } catch { /* Diagnostic failure must preserve the original cleanup error. */ }
        throw error;
      };
      if (snapshot.error) rejectSnapshot("ps-error");
      if (snapshot.status !== 0) rejectSnapshot("ps-nonzero");
      const rows = snapshot.stdout.split("\n").filter((line) => line.trim()).map((line) => line.match(/^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s*$/));
      if (rows.some((row) => !row)) rejectSnapshot("malformed-row");
      const members = rows.filter((row) => Number(row![2]) === child.pid);
      if (members.some((row) => Number(row![3]) !== process.getuid?.())) rejectSnapshot("foreign-member");
      if (members.some((row) => !row![4].startsWith("Z"))) rejectSnapshot("live-member");
      return members.length > 0;
    }
  };
  const stop = () => {
    stopping ??= (async () => {
      const signal = (name: NodeJS.Signals) => {
        if (!child.pid) return;
        try { process.kill(-child.pid, name); }
        catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error; }
      };
      const wait = async () => {
        const until = Date.now() + 5_000;
        while (alive() && Date.now() < until) await new Promise((resolve) => setTimeout(resolve, 50));
      };
      const reap = () => new Promise<void>((resolve) => {
        const timer = setTimeout(resolve, 5_000);
        void exited.then(() => { clearTimeout(timer); resolve(); });
      });
      signal("SIGTERM");
      await reap();
      await wait();
      if (alive()) { signal("SIGKILL"); await reap(); await wait(); }
      if (alive()) throw new Error("Owned repair fixture process group did not stop");
      rmSync(dir, { recursive: true, force: true });
    })();
    return stopping;
  };
  try {
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`synthetic repair server did not start: ${output}`)), 180_000);
      child.once("error", (error) => { clearTimeout(timer); reject(error); });
      child.once("exit", (code) => { clearTimeout(timer); reject(new Error(`synthetic repair server exited ${code}: ${output}`)); });
      child.stdout.on("data", (data: Buffer) => {
        output += data.toString();
        if (output.includes(`careerbrew web app: ${base}/`)) { clearTimeout(timer); resolve(); }
      });
      child.stderr.on("data", (data: Buffer) => { output += data.toString(); });
    });
  } catch (error) {
    await stop();
    throw error;
  }
  return { base, target, link, ghCalls, original, reset, stop };
}
