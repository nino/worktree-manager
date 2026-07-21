import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@shared": resolve(__dirname, "src/shared"),
      "@renderer": resolve(__dirname, "src/renderer/src"),
    },
  },
  test: {
    // happy-dom for the renderer journey tests; the pure node/parser tests run
    // fine under it too (Node built-ins remain available).
    environment: "happy-dom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    setupFiles: ["./src/renderer/src/test/setup.ts"],
  },
});
