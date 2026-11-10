import { z } from "zod";
import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { normaliseText } from "../text.js";
import { registerTool, type ToolContext } from "./register.js";

export function registerProfileTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "get_profile",
    title: "Get student profile",
    description:
      "The signed-in student's profile: name, programme, department, section, year and batch. " +
      "Personal contact fields (email, phone) are omitted unless includePersonal is true.",
    shape: {
      includePersonal: z
        .boolean()
        .optional()
        .describe("Include email and phone. Default false"),
    },
    async handler(args, { client }) {
      const [user, profile] = await Promise.all([client.auth.me(), client.profile()]);
      const base = {
        ukid: user.ukid ?? null,
        name: normaliseText(user.name),
        registrationId: user.registrationId ?? null,
        programmeName: normaliseText(user.programmeName),
        departmentName: normaliseText(user.departmentName),
        sectionName: user.sectionName ?? null,
        year: user.year ?? null,
        batchYear: profile.batchYear,
      };
      if (args.includePersonal) {
        return { ...base, email: user.email ?? null, phone: user.phone ?? null };
      }
      return base;
    },
  });
}
