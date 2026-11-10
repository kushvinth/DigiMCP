import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { QuizSummary } from "fijii";
import { normaliseText } from "../text.js";
import { registerTool, type ToolContext } from "./register.js";

function summariseQuiz(q: QuizSummary) {
  return {
    quizId: q.id,
    title: normaliseText(q.title),
    status: q.status ?? null,
    startTime: q.quizStartTime ?? null,
    endTime: q.quizEndTime ?? null,
    duration: q.quizDuration ?? null,
    maxMarks: q.maxMarks ?? null,
    attemptsCount: q.attemptsCount ?? null,
  };
}

export function registerQuizTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "list_quizzes",
    title: "List quizzes for a class",
    description:
      "Quizzes (Quiz V2) for one class, optionally filtered by status. " +
      "Note: quiz data may be empty on some accounts/tenants — treat results as provisional.",
    shape: {
      classId: z.number().int().describe("Class id from list_classes"),
      status: z.enum(["ONGOING", "SCHEDULED", "CLOSED"]).optional(),
      search: z.string().optional().describe("Name search"),
    },
    async handler(args, { client }) {
      const quizzes = await client.quizzes.all(Number(args.classId), {
        status: args.status as "ONGOING" | "SCHEDULED" | "CLOSED" | undefined,
        search: args.search as string | undefined,
      });
      return { classId: args.classId, count: quizzes.length, quizzes: quizzes.map(summariseQuiz) };
    },
  });
}
