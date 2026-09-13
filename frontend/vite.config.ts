import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

// Tauri expects assets at fixed paths; clear base, default port 5173.
//
// Multi-page build — EVERY window declared in src-tauri/tauri.conf.json
// must have a matching rollup input here, otherwise the HTML file is
// missing from dist/ and the window shows a WebView2 error page in
// release builds. (Dev mode hides this: the Vite server serves any HTML
// file on demand, so a missing rollup input only breaks production.)
//
//   tauri.conf.json window  ->  rollup input
//   main      index.html    ->  main
//   setup     setup.html    ->  setup
//   settings  settings.html ->  settings
//   sidebar   sidebar.html  ->  sidebar
//   loading   loading.html  ->  loading
//
// Silero VAD files (model + worklet) live in public/ so Vite serves them
// as-is without any transformation. ONNX WASM runtime is loaded from CDN
// to avoid the known Vite+onnxruntime-web dynamic import incompatibility.
export default defineConfig({
  plugins: [react()],
  base: "./",
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    headers: {
      "Cache-Control": "no-cache, no-store, must-revalidate",
    },
  },
  envPrefix: ["VITE_"],
  build: {
    target: "es2022",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        setup: resolve(__dirname, "setup.html"),
        settings: resolve(__dirname, "settings.html"),
        sidebar: resolve(__dirname, "sidebar.html"),
        architect: resolve(__dirname, "architect.html"),
        loading: resolve(__dirname, "loading.html"),
        prList: resolve(__dirname, "pr-list.html"),
        settingsSidebar: resolve(__dirname, "settings-sidebar.html"),
      },
    },
  },
});
