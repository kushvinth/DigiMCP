import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { Classroom } from "fijii";
import { normaliseText } from "../text.js";
import { registerTool, type ToolContext } from "./register.js";

function summariseClass(c: Classroom) {
  return {
    classId: c.id,
    courseId: c.courseId,
    courseName: normaliseText(c.courseName),
    courseCode: normaliseText(c.courseCode) || null,
    className: normaliseText(c.className),
    componentType: normaliseText(c.courseComponentTypeName),
    credits: c.courseCredits,
  };
}

export function registerClassTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "list_classes",
    title: "List classes",
    description:
      "List every class (course component) the signed-in student is enrolled in. " +
      "A course may have separate lecture/practical/tutorial entries with different classIds. " +
      "Use the returned classId with the other tools.",
    shape: {},
    async handler(_args, { client }) {
      const classes = await client.classes.list();
      return { count: classes.length, classes: classes.map(summariseClass) };
    },
  });

  registerTool(server, ctx, {
    name: "find_class",
    title: "Find class by name or code",
    description:
      "Resolve a course name or course code (case-insensitive substring) to classIds. " +
      "Useful when you know the subject but not the numeric id.",
    shape: {
      query: z.string().min(1).describe("Course name or code fragment, e.g. 'Physics' or '25MA'"),
    },
    async handler(args, { client }) {
      const q = normaliseText(String(args.query)).toLowerCase();
      const classes = await client.classes.list();
      const matches = classes.filter((c) => {
        const hay = `${normaliseText(c.courseName)} ${normaliseText(c.courseCode)} ${normaliseText(
          c.className,
        )}`.toLowerCase();
        return hay.includes(q);
      });
      return { query: args.query, count: matches.length, classes: matches.map(summariseClass) };
    },
  });
}
