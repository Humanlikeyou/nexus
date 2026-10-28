#!/usr/bin/env node
/**
 * npm run test:e2e: vite build, then Playwright (Chromium) against `vite preview`
 * on a free 127.0.0.1 port. Extra args go to `playwright test`.
 */
import { spawnSync } from "node:child_process";
import { createServer } from "node:net";

const run = (cmd, args, env = process.env) => {
  const r = spawnSync(cmd, args, { stdio: "inherit", env });
  if (r.status !== 0) process.exit(r.status ?? 1);
};

const freePort = () =>
  new Promise((resolve, reject) => {
    const srv = createServer();
    srv.unref();
    srv.on("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });

run("npx", ["vite", "build"]);
const port = process.env.E2E_PORT || String(await freePort());
run("npx", ["playwright", "test", ...process.argv.slice(2)], { ...process.env, E2E_PORT: port });
