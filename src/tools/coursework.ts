import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { ClassResource, ModuleWithComponents, Session } from "fijii";
import { normaliseText } from "../text.js";
import { registerTool, type ToolContext } from "./register.js";

function summariseSession(s: Session) {
  return {
    sessionId: s.id,
    title: normaliseText(s.title),
    type: s.type,
    order: s.sessionOrder ?? null,
    isCompleted: s.isCompleted ?? null,
    resourceCount: s.resourceCount ?? null,
  };
}

function summariseResource(r: ClassResource) {
  return {
    mediaId: r.mediaId,
    mediaName: r.mediaName,
    mediaType: r.mediaType,
    moduleTitle: r.moduleTitle,
    sessionName: r.sessionName,
    weblink: r.weblink ?? null,
    mediaUrl: r.mediaUrl ?? null,
  };
}

export function registerCourseworkTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "list_coursework",
    title: "List course modules and sessions",
    description:
      "Course structure for one class: modules (units) with their sessions, and a flattened resource " +
      "summary. Media URLs are signed and expire in roughly an hour.",
    shape: {
      classId: z.number().int().describe("Class id from list_classes"),
    },
    async handler(args, { client }) {
      const classId = Number(args.classId);
      const [modules, resources] = await Promise.all([
        client.classes.modules(classId),
        client.classes.resources(classId).catch(() => [] as ClassResource[]),
      ]);
      return {
        classId,
        modules: modules.map((m: ModuleWithComponents) => ({
          module: normaliseText(m.module.module),
          title: normaliseText(m.module.title),
          order: m.module.order ?? null,
          sessions: m.componentsList
            .filter((c) => c.componentType === "SESSION")
            .map((c) => summariseSession(c.componentObject as Session)),
        })),
        resources: resources.map(summariseResource),
      };
    },
  });
}
