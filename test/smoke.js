// Broad smoke test: spawn the built server over stdio, handshake, list tools,
// and (if a token is present) call a few read-only tools end-to-end.
//
// Usage: node test/smoke.js
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const bin = join(here, "..", "dist", "index.js");

const env = { ...process.env };
if (!env.DIGII_BASE_URL) env.DIGII_BASE_URL = "https://institute.example.com";
const live = Boolean(process.env.DIGII_TOKEN);

const proc = spawn(process.execPath, [bin], { env, stdio: ["pipe", "pipe", "inherit"] });

let buf = "";
const waiters = new Map();
proc.stdout.on("data", (chunk) => {
  buf += chunk.toString();
  let idx;
  while ((idx = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, idx).trim();
    buf = buf.slice(idx + 1);
    if (!line) continue;
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      continue;
    }
    const w = waiters.get(msg.id);
    if (w) {
      waiters.delete(msg.id);
      w(msg);
    }
  }
});

let id = 0;
function rpc(method, params) {
  const mid = ++id;
  proc.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: mid, method, params }) + "\n");
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${method} timed out`)), 15_000);
    waiters.set(mid, (m) => {
      clearTimeout(timer);
      if (m.error) reject(new Error(`${method}: ${JSON.stringify(m.error)}`));
      else resolve(m.result);
    });
  });
}

function fail(msg) {
  console.error("✗", msg);
  proc.kill();
  process.exit(1);
}

try {
  const init = await rpc("initialize", {
    protocolVersion: "2024-11-05",
    capabilities: {},
    clientInfo: { name: "smoke", version: "0" },
  });
  console.log("✓ initialize:", init.serverInfo.name, init.serverInfo.version);

  const { tools } = await rpc("tools/list", {});
  const names = tools.map((t) => t.name);
  console.log(`✓ tools/list: ${names.length} tools → ${names.join(", ")}`);

  const expected = [
    "list_classes", "find_class", "pending_assignments", "list_assignments", "get_assignment",
    "list_terms", "get_attendance", "get_gradebook", "list_coursework", "list_quizzes",
    "list_discussions", "get_discussion", "institution_info", "get_profile",
  ];
  const missing = expected.filter((n) => !names.includes(n));
  if (missing.length) fail(`missing tools: ${missing.join(", ")}`);

  if (!live) {
    console.log("• no DIGII_TOKEN set — skipping live tool calls");
  } else {
    console.log("• DIGII_TOKEN set — calling read-only tools live");
    for (const name of ["institution_info", "get_profile", "list_classes"]) {
      const res = await rpc("tools/call", { name, arguments: {} });
      if (res.isError) fail(`${name} returned error: ${res.content[0]?.text}`);
      console.log(`✓ ${name}:`, res.content[0]?.text.slice(0, 120).replace(/\s+/g, " "), "…");
    }
  }

  console.log("✓ smoke passed");
  proc.kill();
} catch (err) {
  fail(err.message);
}
