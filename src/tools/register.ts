import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ZodRawShape } from "zod";
import type { Fijii } from "fijii";
import { toToolError, textResult, type ToolResult } from "../errors.js";
import { normaliseText } from "../text.js";

export interface ToolContext {
  client: Fijii;
}

/** A tool definition: description, zod shape, and a handler that returns data. */
export interface ToolDef {
  name: string;
  title: string;
  description: string;
  shape: ZodRawShape;
  handler: (args: Record<string, unknown>, ctx: ToolContext) => Promise<unknown>;
}

/**
 * Register a tool so that thrown errors become actionable results rather than
 * crashing the server. Successful data is serialised as JSON text.
 */
export function registerTool(server: McpServer, ctx: ToolContext, def: ToolDef): void {
  server.registerTool(
    def.name,
    {
      title: def.title,
      description: def.description,
      inputSchema: def.shape,
    },
    async (args: Record<string, unknown>): Promise<ToolResult> => {
      try {
        const data = await def.handler(args, ctx);
        return textResult(data);
      } catch (err) {
        return toToolError(err);
      }
    },
  );
}

// ---------------------------------------------------------------------------
// Shared normalisers
// ---------------------------------------------------------------------------

/** Clean a possibly-nbsp-laden portal string. */
export { normaliseText };
