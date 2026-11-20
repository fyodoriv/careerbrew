export function isHealthRead(method: string, url: URL, origin: string): boolean {
  return method === "GET" && url.origin === origin && url.pathname === "/api/feed/health" && url.search === "";
}
