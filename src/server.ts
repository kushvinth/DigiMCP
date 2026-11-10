import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { Fijii } from "fijii";
import { registerClassTools } from "./tools/classes.js";
import { registerAssignmentTools } from "./tools/assignments.js";
import { registerAttendanceTools } from "./tools/attendance.js";
import { registerGradebookTools } from "./tools/gradebook.js";
import { registerCourseworkTools } from "./tools/coursework.js";
import { registerQuizTools } from "./tools/quizzes.js";
import { registerDiscussionTools } from "./tools/discussions.js";
import { registerInstitutionTools } from "./tools/institution.js";
import { registerProfileTools } from "./tools/profile.js";

/** Build the MCP server with every read-only tool registered. */
export function createServer(client: Fijii): McpServer {
  const server = new McpServer({ name: "fijii-mcp", version: "0.1.0" });
  const ctx = { client };

  registerClassTools(server, ctx);
  registerAssignmentTools(server, ctx);
  registerAttendanceTools(server, ctx);
  registerGradebookTools(server, ctx);
  registerCourseworkTools(server, ctx);
  registerQuizTools(server, ctx);
  registerDiscussionTools(server, ctx);
  registerInstitutionTools(server, ctx);
  registerProfileTools(server, ctx);

  return server;
}
