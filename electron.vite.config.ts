import { resolve } from "node:path";
import { defineConfig, externalizeDepsPlugin } from "electron-vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin()],
    // `@shared` holds code shared with the renderer (types + pure helpers).
    // Aliased here too so runtime value imports (e.g. INIT_COMMAND_ID,
    // slugifyBranch) resolve — type-only imports are erased and never needed it.
    resolve: { alias: { "@shared": resolve(__dirname, "src/shared") } },
    build: { rollupOptions: { input: { index: resolve(__dirname, "src/main/index.ts") } } },
  },
  preload: {
    plugins: [externalizeDepsPlugin()],
    resolve: { alias: { "@shared": resolve(__dirname, "src/shared") } },
    build: {
      rollupOptions: {
        input: { index: resolve(__dirname, "src/preload/index.ts") },
        output: { format: "es", entryFileNames: "[name].mjs" },
      },
    },
  },
  renderer: {
    root: resolve(__dirname, "src/renderer"),
    resolve: {
      alias: {
        "@shared": resolve(__dirname, "src/shared"),
        "@renderer": resolve(__dirname, "src/renderer/src"),
      },
    },
    build: { rollupOptions: { input: { index: resolve(__dirname, "src/renderer/index.html") } } },
    plugins: [react()],
  },
});
