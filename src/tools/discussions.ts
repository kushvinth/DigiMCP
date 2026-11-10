import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { Discussion } from "fijii";
import { normaliseText } from "../text.js";
import { registerTool, type ToolContext } from "./register.js";

function summariseDiscussion(d: Discussion) {
  return {
    discussionId: d.id,
    title: normaliseText(d.title),
    classId: d.classId ?? null,
    createdTimestamp: d.createdTimestamp ?? null,
    commentCount: d.commentCount ?? null,
    upvoteCount: d.upvoteCount ?? null,
  };
}

export function registerDiscussionTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "list_discussions",
    title: "List discussions for a class",
    description: "Class forum threads with comment and upvote counts.",
    shape: {
      classId: z.number().int().describe("Class id from list_classes"),
      page: z.number().int().min(1).optional().describe("Default 1"),
      perPage: z.number().int().min(1).max(100).optional().describe("Default 20 (always sent)"),
    },
    async handler(args, { client }) {
      const page = args.page === undefined ? 1 : Number(args.page);
      // perPage is mandatory on the backend — always pass a value.
      const perPage = args.perPage === undefined ? 20 : Number(args.perPage);
      const data = await client.discussions.list(Number(args.classId), page, perPage);
      return {
        classId: args.classId,
        count: data.count ?? data.discussions?.length ?? 0,
        canUserCreateDiscussionForum: data.canUserCreateDiscussionForum ?? false,
        discussions: (data.discussions ?? []).map(summariseDiscussion),
      };
    },
  });

  registerTool(server, ctx, {
    name: "get_discussion",
    title: "Get a discussion thread",
    description: "Full detail for a single discussion thread, including its description.",
    shape: {
      discussionId: z.number().int(),
    },
    async handler(args, { client }) {
      const d = await client.discussions.detail(Number(args.discussionId));
      return {
        ...summariseDiscussion(d),
        description: d.description ?? null,
      };
    },
  });
}
