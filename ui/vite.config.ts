import { defineConfig } from "vitest/config";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  // `tauri dev` loads the page from here; a fixed port, or it would not find it.
  server: { port: 1420, strictPort: true },
  clearScreen: false,
  build: {
    // Every webview Tauri 2 runs in is recent enough for this.
    target: "es2022",
    // One file each: nothing is lazy, and a single request is fastest from
    // Tauri's in-memory asset protocol.
    modulePreload: false,
    cssCodeSplit: false,
  },
  test: {
    environment: "jsdom",
  },
});
