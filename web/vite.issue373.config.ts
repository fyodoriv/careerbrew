import { defineConfig, mergeConfig } from "vite";
import base from "./vite.config";

const run = process.env.ISSUE373_RUN;
if (!run) throw new Error("BLOCKED: owned373 output root required");
export default mergeConfig(base, defineConfig({
  envDir: false,
  publicDir: false,
  cacheDir: `${run}/vite-cache`,
  build: { outDir: `${run}/build`, emptyOutDir: false },
}));
