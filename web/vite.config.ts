import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  base: "./",
  plugins: [react()],
  resolve: {
    alias: {
      "@abstractuic/panel-chat": resolve(__dirname, "../../abstractuic/panel-chat/src"),
    },
  },
  server: {
    fs: {
      allow: [resolve(__dirname), resolve(__dirname, "../../abstractuic")],
    },
  },
  test: {
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    exclude: ["thin_client/**", "node_modules/**", "dist/**"],
  },
});
