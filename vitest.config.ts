import { defineConfig, mergeConfig } from "vitest/config";
import viteConfig from "./vite.config";

export default defineConfig(async (env) =>
  mergeConfig(await (viteConfig as any)(env), {
    test: {
      environment: "jsdom",
      setupFiles: ["./test/setup.ts"],
      include: ["src/**/*.test.ts?(x)", "src-console/**/*.test.ts?(x)", "test/**/*.test.ts?(x)"],
      exclude: ["**/node_modules/**", "src-console/neuro/*.test.mjs", "src-tauri/**"],
      restoreMocks: true,
      clearMocks: true,
      passWithNoTests: true,
    },
  }),
);
