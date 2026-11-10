import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { normaliseText } from "../text.js";
import { registerTool, type ToolContext } from "./register.js";

export function registerGradebookTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "get_gradebook",
    title: "Get gradebook for a class",
    description:
      "The signed-in student's own assessment result for one class: grades, component results and " +
      "published result. Students can only ever see their own record.",
    shape: {
      classId: z.number().int().describe("Class id from list_classes"),
    },
    async handler(args, { client }) {
      const r = await client.gradebook.assessmentResult(Number(args.classId));
      return {
        classId: args.classId,
        courseName: normaliseText(r.student?.programmeName),
        rollNumber: r.rollNumber ?? null,
        student: {
          ukid: r.student?.ukid ?? null,
          name: normaliseText(r.student?.name),
          section: r.student?.sectionName ?? null,
          year: r.student?.yearName ?? null,
        },
        result: r.result ?? null,
        grades: r.grades ?? [],
        assessmentComponentResults: r.assessmentComponentResults ?? [],
      };
    },
  });
}
