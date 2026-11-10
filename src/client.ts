import { Fijii } from "fijii";
import type { Config } from "./config.js";
import { tokenProvider } from "./token.js";

/**
 * Build the SDK client.
 *
 * The token provider is a function so rotation is picked up on the next request.
 * The request logger writes only method + pathname to stderr — never the token
 * or query strings, and never to stdout (which MCP owns).
 */
export function createClient(config: Config): Fijii {
  return new Fijii({
    baseUrl: config.baseUrl,
    timeZone: config.timeZone,
    timeoutMs: config.timeoutMs,
    token: () => tokenProvider(),
    onRequest: ({ method, url }) => {
      try {
        const { pathname } = new URL(url);
        console.error(`→ ${method} ${pathname}`);
      } catch {
        console.error(`→ ${method} (unparseable url)`);
      }
    },
  });
}
