import { defineConfig } from "vite";

// Tauri desktop frontend. Dev server on a fixed port so tauri.conf.json's
// devUrl matches; build emits static assets consumed by the Rust bundler.
export default defineConfig({
  root: ".",
  clearScreen: false,
  server: {
    port: 5174,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2022",
  },
});
