import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "node:path";

// Two entry documents. The overlay is deliberately separate from the main
// window: it appears on every dictation and must paint in single-digit
// milliseconds, so it must not share the settings bundle.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Rust rebuilds are Cargo's business; watching them just thrashes Vite.
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },
  build: {
    target: "es2022",
    // Tauri ships its own webview, so there is no legacy browser to support
    // and no reason to pay for the transforms.
    minify: "esbuild",
    sourcemap: true,
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        overlay: resolve(__dirname, "overlay.html"),
      },
    },
  },
});
