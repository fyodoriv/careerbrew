/// <reference types="vitest" />
import { availableParallelism } from "node:os";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Dev server proxies the API to `careerbrew serve` (default port 8765); the built app is served by careerbrew itself.
export default defineConfig({
  plugins: [react()],
  server: { proxy: { "/api": "http://127.0.0.1:8765" } },
  test: { maxWorkers: Math.max(1, Math.min(2, availableParallelism())), minWorkers: 1, environment: "jsdom", include: ["src/**/*.test.{ts,tsx}"], setupFiles: ["./src/test-setup.ts"] },
});
