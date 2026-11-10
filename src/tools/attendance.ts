import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ComponentSummary, CourseSummary, TermSummary } from "fijii";
import { registerTool, type ToolContext } from "./register.js";

function summariseCourse(c: CourseSummary) {
  return {
    courseId: c.courseId,
    courseName: c.courseName,
    courseCode: c.courseCode,
    present: c.present,
    absent: c.absent,
    total: c.total,
    percentage: c.percentage,
  };
}

function summariseComponent(c: ComponentSummary) {
  return {
    classId: c.classId,
    componentType: c.componentType,
    className: c.className,
    present: c.present,
    absent: c.absent,
    total: c.total,
    percentage: c.percentage,
    unmarkedLessons: c.unmarkedLessons,
  };
}

function summariseTerm(t: TermSummary, withComponents: boolean) {
  return {
    termId: t.termId,
    termName: t.term?.name ?? null,
    present: t.present,
    absent: t.absent,
    total: t.total,
    percentage: t.percentage,
    approvedLeaves: t.approvedLeaves,
    unapprovedLeaves: t.unapprovedLeaves,
    courses: t.courses.map((c) => ({
      ...summariseCourse(c),
      components: withComponents ? c.components.map(summariseComponent) : undefined,
    })),
    note: "total = present + absent; unmarked and 'do not consider' lessons are excluded.",
  };
}

export function registerAttendanceTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "list_terms",
    title: "List academic terms",
    description:
      "Academic terms for the student's programme and batch. Use a term's id with get_attendance.",
    shape: {},
    async handler(_args, { client }) {
      const terms = await client.attendance.terms();
      return {
        count: terms.length,
        terms: terms.map((t) => ({
          id: t.id,
          name: t.name,
          starts: t.starts,
          ends: t.ends,
          active: t.active,
          past: t.past,
          future: t.future,
        })),
      };
    },
  });

  registerTool(server, ctx, {
    name: "get_attendance",
    title: "Attendance summary",
    description:
      "Attendance for a term (defaults to the current term): overall totals, per-course and per-component " +
      "breakdowns. `unmarkedLessons` shows lessons not yet marked. " +
      "Counting rule: total = present + absent.",
    shape: {
      termId: z.number().int().optional().describe("Term id from list_terms; omit for current term"),
      includeComponents: z
        .boolean()
        .optional()
        .describe("Include per-component (lecture/practical) breakdown. Default false"),
    },
    async handler(args, { client }) {
      const summary = await client.attendanceSummary(
        args.termId === undefined ? undefined : Number(args.termId),
      );
      return summariseTerm(summary, Boolean(args.includeComponents));
    },
  });
}
