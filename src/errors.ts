import { DigiiError } from "fijii";

export interface ToolResult {
  [key: string]: unknown;
  content: Array<{ type: "text"; text: string }>;
  isError?: boolean;
}

/** A readable description of a failure for the calling Agent. */
export function describeError(err: unknown): string {
  if (err instanceof DigiiError) {
    switch (err.kind) {
      case "unauthorized":
        return "Session expired or token missing — refresh DIGII_TOKEN and try again.";
      case "forbidden":
        return "This account does not have access to that feature.";
      case "not-permitted":
        return `Not permitted${err.code ? ` (${err.code})` : ""}.`;
      case "not-found":
        return "That item no longer exists, or the route has changed.";
      case "spa-fallback":
        return "Unknown endpoint — the portal returned a web page instead of data.";
      case "network":
        return "Network unavailable or timed out — try again.";
      case "parse":
        return `Unusable response from the portal: ${err.detail}`;
      case "http":
        return `Server error ${err.status ?? ""}: ${err.detail}`.trim();
      default:
        return err.detail;
    }
  }
  if (err instanceof Error) return err.message;
  return String(err);
}

/** Turn any thrown value into a tool result that the Agent can act on. */
export function toToolError(err: unknown): ToolResult {
  return {
    content: [{ type: "text", text: describeError(err) }],
    isError: true,
  };
}

/** Serialise a value as pretty JSON in a successful tool result. */
export function textResult(value: unknown): ToolResult {
  return { content: [{ type: "text", text: JSON.stringify(value, null, 2) }] };
}
