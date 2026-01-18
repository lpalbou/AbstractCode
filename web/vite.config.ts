import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  base: "./",
  plugins: [react()],
  resolve: {
    alias: {
      "@abstractuic/monitor-flow": resolve(__dirname, "../../abstractuic/monitor-flow/src"),
      "@abstractuic/panel-chat": resolve(__dirname, "../../abstractuic/panel-chat/src"),
      "@abstractutils/monitor-gpu": resolve(__dirname, "../../abstractuic/monitor-gpu/src"),
    },
  },
  server: {
    host: "0.0.0.0",
    allowedHosts: true,
    strictPort: false,
    cors: true,
    fs: {
      allow: [resolve(__dirname), resolve(__dirname, "../../abstractuic")],
    },
    proxy: {
      "/api": {
        target: "http://localhost:8080",
        changeOrigin: true,
        ws: true,
        secure: false,
      },
    },
  },
  test: {
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    exclude: ["thin_client/**", "node_modules/**", "dist/**"],
  },
});
