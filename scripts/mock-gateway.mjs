#!/usr/bin/env node
/**
 * Mock Gateway Server for Electron outbox pipeline testing.
 *
 * Usage:
 *   node scripts/mock-gateway.mjs                  # normal mode (ACK all)
 *   node scripts/mock-gateway.mjs --partial         # ACK only first event per batch
 *   node scripts/mock-gateway.mjs --no-ack          # return 200 but no received[] field
 *   node scripts/mock-gateway.mjs --fail            # return 500 on POST /events
 *   node scripts/mock-gateway.mjs --port 9999       # custom port (default 18789)
 *
 * Endpoints:
 *   GET  /health   -> { "ok": true }
 *   POST /events   -> { "ok": true, "received": [...event_ids...] }
 */

import { createServer } from "node:http";

const args = process.argv.slice(2);
const PORT = getArg("--port", 18789);
const MODE_PARTIAL = args.includes("--partial");
const MODE_NO_ACK = args.includes("--no-ack");
const MODE_FAIL = args.includes("--fail");

function getArg(flag, fallback) {
  const idx = args.indexOf(flag);
  if (idx !== -1 && args[idx + 1]) return parseInt(args[idx + 1], 10);
  return fallback;
}

let requestCount = 0;
let totalEventsReceived = 0;

const server = createServer((req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`);

  // GET /health
  if (req.method === "GET" && url.pathname === "/health") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ ok: true }));
    console.log(`[mock-gw] GET /health -> 200`);
    return;
  }

  // POST /events
  if (req.method === "POST" && url.pathname === "/events") {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      requestCount++;
      let events;
      try {
        events = JSON.parse(body);
      } catch {
        res.writeHead(400, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: "Invalid JSON" }));
        console.log(`[mock-gw] POST /events -> 400 (bad JSON)`);
        return;
      }

      if (!Array.isArray(events)) {
        res.writeHead(400, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: "Expected array" }));
        console.log(`[mock-gw] POST /events -> 400 (not array)`);
        return;
      }

      const eventIds = events.map((e) => e.id).filter(Boolean);
      totalEventsReceived += eventIds.length;

      // --fail mode: return 500
      if (MODE_FAIL) {
        res.writeHead(500, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ error: "Simulated server error" }));
        console.log(
          `[mock-gw] POST /events -> 500 (--fail) events=${eventIds.length}`
        );
        return;
      }

      // --no-ack mode: return 200 but without received field
      if (MODE_NO_ACK) {
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ ok: true }));
        console.log(
          `[mock-gw] POST /events -> 200 (--no-ack, no received[]) events=${eventIds.length}`
        );
        return;
      }

      // --partial mode: only ACK the first event
      if (MODE_PARTIAL) {
        const partial = eventIds.slice(0, 1);
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ ok: true, received: partial }));
        console.log(
          `[mock-gw] POST /events -> 200 (--partial) acked=${partial.length}/${eventIds.length}`
        );
        return;
      }

      // Normal mode: ACK all
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ ok: true, received: eventIds }));
      console.log(
        `[mock-gw] POST /events -> 200 (full ACK) events=${eventIds.length}`
      );
    });
    return;
  }

  // GET /stats (bonus: check total)
  if (req.method === "GET" && url.pathname === "/stats") {
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        requests: requestCount,
        total_events_received: totalEventsReceived,
      })
    );
    return;
  }

  res.writeHead(404, { "Content-Type": "application/json" });
  res.end(JSON.stringify({ error: "Not found" }));
});

server.listen(PORT, () => {
  const mode = MODE_FAIL
    ? "--fail (500s)"
    : MODE_NO_ACK
      ? "--no-ack (200 without received[])"
      : MODE_PARTIAL
        ? "--partial (ACK first event only)"
        : "normal (full ACK)";
  console.log(`[mock-gw] Listening on http://127.0.0.1:${PORT}`);
  console.log(`[mock-gw] Mode: ${mode}`);
  console.log(`[mock-gw] Endpoints: GET /health, POST /events, GET /stats`);
  console.log(`[mock-gw] Ctrl+C to stop`);
});
