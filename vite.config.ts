import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import crypto from "crypto";

const host = process.env.TAURI_DEV_HOST;

// Build stamp: baked into JS at compile time
const buildHash = crypto.randomBytes(4).toString("hex"); // 8-char hex
const buildTime = new Date().toISOString();

export default defineConfig(async () => ({
  plugins: [react()],
  define: {
    __BUILD_HASH__: JSON.stringify(buildHash),
    __BUILD_TIME__: JSON.stringify(buildTime),
    __APP_VERSION__: JSON.stringify("0.1.0"),
    __TAURI_VERSION__: JSON.stringify("2"),
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@console": path.resolve(__dirname, "./src-console"),
    },
  },
  build: {
    chunkSizeWarningLimit: 750,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("lucide-react")) return "interface-icons";
          return undefined;
        },
      },
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
