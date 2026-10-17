import { resolve } from "node:path";
import { defineConfig } from "vite";

// One HTML page per window. Tauri loads them from the dev server in `tauri dev` and from
// `dist/` in a build.
const ui = resolve(import.meta.dirname, "ui");

export default defineConfig({
  root: ui,
  clearScreen: false,
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  build: {
    outDir: resolve(import.meta.dirname, "dist"),
    emptyOutDir: true,
    target: "safari15",
    rolldownOptions: {
      input: {
        signin: resolve(ui, "signin.html"),
        linked: resolve(ui, "linked.html"),
        popover: resolve(ui, "popover.html"),
      },
    },
  },
});
