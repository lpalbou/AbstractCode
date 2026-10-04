import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import { resolve } from "path";
import { readFileSync } from "fs";
import { appVersionFrom } from "./app_version";
import { loadEnv, type Plugin } from "vite";
// The published runtime is intentionally Node-builtins-only. Its middleware
// is shared here so development cannot bypass the app-cookie session exchange.
import { createGatewayMiddleware } from "./bin/server.js";

function codeGatewayPlugin(env: Record<string, string>): Plugin {
  return {
    name: "abstractcode-gateway-session-proxy",
    configureServer(server) {
      server.middlewares.use(createGatewayMiddleware({ env }));
    },
  };
}

// The app's llms.txt ships in dist/ and is served at /llms.txt (dev too): the
// gateway's `GET /docs/corpus?app=code` reads it from the running app to
// ground the Docs assistant (round 8, R8.3). One file, the repo's own.
const LLMS_TXT = resolve(__dirname, "../llms.txt");
function llmsTxtPlugin(): Plugin {
  return {
    name: "abstractframework-llms-txt",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        if (String(req.url || "").split("?")[0] !== "/llms.txt") return next();
        res.setHeader("Content-Type", "text/plain; charset=utf-8");
        res.end(readFileSync(LLMS_TXT, "utf8"));
      });
    },
    generateBundle() {
      this.emitFile({ type: "asset", fileName: "llms.txt", source: readFileSync(LLMS_TXT, "utf8") });
    },
  };
}

// The About dialog's version: the package.json this build ships as, for the
// app and for tests alike (no runtime fallback; a missing define fails tsc).
const APP_VERSION = appVersionFrom(
  readFileSync(resolve(__dirname, "package.json"), "utf8"),
);

export default defineConfig(({ mode }) => ({
  base: "./",
  define: { __APP_VERSION__: JSON.stringify(APP_VERSION) },
  plugins: [
    react(),
    llmsTxtPlugin(),
    codeGatewayPlugin({
      ...loadEnv(mode, __dirname, ""),
      ...((
        globalThis as unknown as { process?: { env?: Record<string, string> } }
      ).process?.env || {}),
    }),
  ],
  // The @abstractframework/* kit packages resolve from node_modules like any
  // other dependency — see package.json. They were once aliased to a sibling
  // `../../abstractuic` checkout, which silently coupled this build to the
  // layout of the directory ABOVE the repo: the app only built where a
  // matching sibling happened to sit, and CI floated on that repo's default
  // branch (no `ref:`), so a release could ship whatever was on it that day.
  // Consuming the published packages is what makes this app relocatable.
  server: {
    host: "0.0.0.0",
    strictPort: false,
    fs: {
      allow: [resolve(__dirname), resolve(__dirname, "../docs")],
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
}));
