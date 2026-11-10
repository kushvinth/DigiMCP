// Targeted check: a tool call that hits the network with a dummy token must
// return a structured isError result (not crash the server).
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const bin = join(here, "..", "dist", "index.js");

const proc = spawn(process.execPath, [bin], {
  env: { ...process.env, DIGII_BASE_URL: "https://institute.example.com", DIGII_TOKEN: "not-a-jwt" },
  stdio: ["pipe", "pipe", "inherit"],
});

let buf = "";
const waiters = new Map();
proc.stdout.on("data", (c) => {
  buf += c.toString();
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (!line) continue;
    try {
      const m = JSON.parse(line);
      const w = waiters.get(m.id);
      if (w) { waiters.delete(m.id); w(m); }
    } catch {}
  }
});

let id = 0;
function rpc(method, params) {
  const mid = ++id;
  proc.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: mid, method, params }) + "\n");
  return new Promise((res) => waiters.set(mid, res));
}

await rpc("initialize", { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "e", version: "0" } });
const r = await rpc("tools/call", { name: "list_classes", arguments: {} });

const out = r.result ?? r.error;
const isError = r.result?.isError === true;
console.log(isError ? "✓ tool failure surfaced as isError:" : "✗ not an isError:", (r.result?.content?.[0]?.text ?? JSON.stringify(out)).slice(0, 120));
proc.kill();
process.exit(isError ? 0 : 1);
