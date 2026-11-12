// The last response of each view, so a view switch or a reload shows data at once while it refreshes (issue #142).
//
// Lifecycle and scope:
// - `memory`: browser-page scope. One entry per endpoint path (at most MAX_MEMORY); gone on a reload.
// - sessionStorage: tab scope. The browser drops it when the tab closes, but may keep it on disk to restore
//   the session, so each entry is encrypted (AES-GCM) with a key that only the running careerbrew server holds
//   (`/api/tab-key`, server-instance scope). Once that server stops, a kept copy cannot be read. Entry names are
//   hashes, so even the endpoint (a company or page name) stays private. Only what the page showed is kept,
//   never the token. With `web_tab_cache` off the server sends no key and nothing is kept in the tab.
// - `clearCache()` deletes both (tests, and a quota error).
import { api } from "./api";

export interface Entry<T> { data: T; at: number; stale: boolean }

const PREFIX = "careerbrew-cache:";
const KEY_MARK = "careerbrew-cache-key";  // a hash of the key the entries were written with
const MAX_MEMORY = 64;
const MAX_STORED = 48;
const MAX_BYTES = 512_000;  // sessionStorage holds about 5 MB per origin; a bigger view (a long run page) stays in memory only

const memory = new Map<string, Entry<unknown>>();

export function peek<T>(key: string): Entry<T> | undefined {
  return memory.get(key) as Entry<T> | undefined;
}

export function save<T>(key: string, data: T): void {
  const entry: Entry<T> = { data, at: Date.now(), stale: false };
  memory.delete(key);
  memory.set(key, entry);
  while (memory.size > MAX_MEMORY) memory.delete(memory.keys().next().value as string);
  void persist(key, entry);
}

// `/api/changes` moved: what the page keeps is older than the server's data. A stale entry still shows at once
// and says that new data is on the way.
export function markAllStale(): void {
  for (const entry of memory.values()) entry.stale = true;
}

export function clearCache(): void {
  memory.clear();
  tabKey = null;
  [...stored(), KEY_MARK].forEach((name) => tryStorage(() => sessionStorage.removeItem(name)));
}

// ---------- the tab's encrypted copy ----------

interface TabKey { raw: Uint8Array; aes: CryptoKey }
let tabKey: Promise<TabKey | null> | null = null;

function getTabKey(): Promise<TabKey | null> {
  if (tabKey) return tabKey;
  const asked: Promise<TabKey | null> = (async () => {
    const subtle = typeof crypto === "undefined" ? undefined : crypto.subtle;
    if (!subtle) return null;  // not a secure context: memory only
    const { key } = await api.tabKey();
    if (!key) return null;
    const raw = Uint8Array.from(atob(key), (c) => c.charCodeAt(0));
    if (raw.length !== 32) return null;
    const k = { raw, aes: await subtle.importKey("raw", raw, "AES-GCM", false, ["encrypt", "decrypt"]) };
    // A new server (a restart) has a new key: what the tab kept under the old one is unreadable, so drop it.
    const mark = await nameOf(k, "");
    if (tryStorage(() => sessionStorage.getItem(KEY_MARK)) !== mark) {
      stored().forEach((name) => tryStorage(() => sessionStorage.removeItem(name)));
      tryStorage(() => sessionStorage.setItem(KEY_MARK, mark));
    }
    return k;
  })().catch(() => {
    // The call failed (a 401 right after `careerbrew web rotate-token`, a restart): the next read asks again.
    if (tabKey === asked) tabKey = null;
    return null;
  });
  tabKey = asked;
  return asked;
}

const enc = new TextEncoder();
const b64 = (bytes: Uint8Array) => btoa(Array.from(bytes, (b) => String.fromCharCode(b)).join(""));
const unb64 = (text: string) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

function tryStorage<T>(fn: () => T): T | undefined {
  try {
    return fn();
  } catch {
    return undefined;  // storage off, full, or not allowed: the page works from memory
  }
}

function stored(): string[] {
  return tryStorage(() => Array.from({ length: sessionStorage.length }, (_, i) => sessionStorage.key(i) ?? "").filter((k) => k.startsWith(PREFIX))) ?? [];
}

// The entry's name hides its endpoint: a hash of the tab key and the path.
async function nameOf(k: TabKey, key: string): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new Uint8Array([...k.raw, ...enc.encode(key)])));
  return PREFIX + Array.from(digest.slice(0, 16), (b) => b.toString(16).padStart(2, "0")).join("");
}

async function persist<T>(key: string, entry: Entry<T>): Promise<void> {
  try {
    const k = await getTabKey();
    if (!k) return;
    const plain = enc.encode(JSON.stringify({ at: entry.at, data: entry.data }));
    const name = await nameOf(k, key);
    if (plain.length > MAX_BYTES) {
      tryStorage(() => sessionStorage.removeItem(name));
      return;
    }
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const ct = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: enc.encode(key) }, k.aes, plain));
    const names = stored();
    if (names.length >= MAX_STORED && !names.includes(name)) names.forEach((n) => tryStorage(() => sessionStorage.removeItem(n)));
    try {
      sessionStorage.setItem(name, JSON.stringify({ iv: b64(iv), ct: b64(ct) }));
    } catch {
      stored().forEach((n) => tryStorage(() => sessionStorage.removeItem(n)));  // full: start over rather than fail
    }
  } catch {
    // crypto or storage failed: the page keeps working from memory
  }
}

// Read the tab's copy of `key` after a reload. Undefined when there is none, or when it cannot be read
// (another server's key, a changed shape): then the view loads as usual.
export async function restore<T>(key: string): Promise<Entry<T> | undefined> {
  try {
    const k = await getTabKey();
    if (!k) return undefined;
    const name = await nameOf(k, key);
    const item = tryStorage(() => sessionStorage.getItem(name));
    if (!item) return undefined;
    try {
      const { iv, ct } = JSON.parse(item) as { iv: string; ct: string };
      const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: unb64(iv), additionalData: enc.encode(key) }, k.aes, unb64(ct));
      const { at, data } = JSON.parse(new TextDecoder().decode(plain)) as { at: number; data: T };
      const entry: Entry<T> = { data, at, stale: false };
      if (!memory.has(key)) memory.set(key, entry);
      return entry;
    } catch {
      tryStorage(() => sessionStorage.removeItem(name));
      return undefined;
    }
  } catch {
    return undefined;
  }
}
