#!/usr/bin/env node
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { loadConfig } from "./config.js";
import { createClient } from "./client.js";
import { createServer } from "./server.js";

async function main(): Promise<void> {
  const config = loadConfig();
  const client = createClient(config);
  const server = createServer(client);
  const transport = new StdioServerTransport();
  await server.connect(transport);
  // stdout is owned by the MCP transport; only stderr is safe for logs.
  console.error(`fijii-mcp ready (${config.baseUrl})`);
}

main().catch((err) => {
  console.error("fijii-mcp failed to start:", err instanceof Error ? err.message : err);
  process.exit(1);
});
