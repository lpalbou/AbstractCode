import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  base: "./",
  plugins: [react()],
  // The @abstractframework/* kit packages resolve from node_modules like any
  // other dependency — see package.json. They were once aliased to a sibling
  // `../../abstractuic` checkout, which silently coupled this build to the
  // layout of the directory ABOVE the repo: the app only built where a
  // matching sibling happened to sit, and CI floated on that repo's default
  // branch (no `ref:`), so a release could ship whatever was on it that day.
  // Consuming the published packages is what makes this app relocatable.
  server: {
    host: "0.0.0.0",
    allowedHosts: true,
    strictPort: false,
    cors: true,
    fs: {
      allow: [resolve(__dirname)],
    },
    proxy: {
      "/api": {
        target: "http://localhost:8081",
        changeOrigin: true,
        ws: true,
        secure: false,
      },
    },
  },
  test: {
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    exclude: ["thin_client/**", "node_modules/**", "dist/**"],
    server: {
      // The published kit ships components that import their own CSS
      // (e.g. monitor-flow's AgentCyclesPanel). Vitest externalizes
      // node_modules by default and hands .css to Node's ESM loader, which
      // throws `Unknown file extension ".css"`. Inlining the kit routes
      // those imports through Vite's transform, which handles CSS.
      deps: { inline: [/@abstractframework\//] },
    },
  },
});
