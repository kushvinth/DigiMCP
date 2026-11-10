import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { Assignment, PendingAssignment } from "fijii";
import { describeError } from "../errors.js";
import { isoOrNull, normaliseText } from "../text.js";
import { registerTool, type ToolContext } from "./register.js";

function summariseAssignment(a: Assignment) {
  return {
    assignmentId: a.id,
    name: normaliseText(a.name),
    type: a.type,
    dueDate: a.dueDate,
    startDate: a.startDate,
    isSubmitted: a.isSubmitted,
    dueDatePassed: a.dueDatePassed,
    isRejected: a.isRejected,
    isGraded: a.isGraded,
    grade: a.grade,
    submissionType: a.submissionType,
  };
}

function summarisePending(a: PendingAssignment) {
  return {
    assignmentId: a.id,
    name: normaliseText(a.name),
    classId: a.classId,
    classLabel: normaliseText(a.classLabel),
    dueDate: a.dueDate,
    dueAt: isoOrNull(a.dueAt),
    daysUntilDue: a.daysUntilDue === null ? null : Math.round(a.daysUntilDue * 10) / 10,
    isRejected: a.isRejected,
  };
}

export function registerAssignmentTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "pending_assignments",
    title: "Pending assignments across all classes",
    description:
      "Every assignment still needing attention across all classes, sorted by due date. " +
      "Includes overdue-but-open and rejected submissions by default. `daysUntilDue` is negative when overdue. " +
      "Per-class fetch failures are reported under `warnings` instead of failing the whole call.",
    shape: {
      classIds: z.array(z.number().int()).optional().describe("Restrict to these classIds"),
      dueWithinDays: z.number().optional().describe("Only those due within N days"),
      includeOverdue: z.boolean().optional().describe("Default true"),
      includeRejected: z.boolean().optional().describe("Default true"),
    },
    async handler(args, { client }) {
      const warnings: Array<{ classId: number; error: string }> = [];
      const pending = await client.pendingAssignments({
        classIds: args.classIds as number[] | undefined,
        dueWithinDays: args.dueWithinDays as number | undefined,
        includeOverdue: args.includeOverdue as boolean | undefined,
        includeRejected: args.includeRejected as boolean | undefined,
        onError: (classId, error) => warnings.push({ classId, error: describeError(error) }),
      });
      return {
        count: pending.length,
        assignments: pending.map(summarisePending),
        ...(warnings.length ? { warnings } : {}),
      };
    },
  });

  registerTool(server, ctx, {
    name: "list_assignments",
    title: "List assignments for a class",
    description:
      "All assignments for one class, split into upcoming / previous / draft buckets with totals.",
    shape: {
      classId: z.number().int().describe("Class id from list_classes"),
    },
    async handler(args, { client }) {
      const buckets = await client.classes.allAssignments(Number(args.classId));
      return {
        classId: args.classId,
        totals: buckets.totals,
        upcoming: buckets.upcoming.map(summariseAssignment),
        previous: buckets.previous.map(summariseAssignment),
        draft: buckets.draft.map(summariseAssignment),
      };
    },
  });

  registerTool(server, ctx, {
    name: "get_assignment",
    title: "Get assignment detail",
    description:
      "Full detail for one assignment: description, resources, submissions and rubric. " +
      "Note: media URLs are signed and expire in roughly an hour.",
    shape: {
      assignmentId: z.number().int(),
      classId: z.number().int().describe("The class that owns the assignment"),
    },
    async handler(args, { client }) {
      const a = await client.classes.assignment(Number(args.assignmentId), Number(args.classId));
      return {
        assignmentId: a.id,
        name: normaliseText(a.name),
        description: a.description,
        type: a.type,
        dueDate: a.dueDate,
        submittedOn: a.submittedOn,
        isSubmitted: a.isSubmitted,
        isGraded: a.isGraded,
        isRejected: a.isRejected,
        grade: a.grade,
        remark: a.remark,
        minimumMarks: a.minimumMarks,
        maximumMarks: a.maximumMarks,
        resources: (a.resources ?? []).map((r) => ({
          mediaId: r.mediaId,
          mediaName: r.mediaName,
          mediaType: r.mediaType,
          weblink: r.weblink ?? null,
          mediaUrl: r.mediaUrl ?? null,
        })),
        submissionCount: a.submissions?.length ?? 0,
      };
    },
  });
}
