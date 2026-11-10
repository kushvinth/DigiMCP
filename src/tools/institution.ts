import type { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { describeError } from "../errors.js";
import { registerTool, type ToolContext } from "./register.js";

export function registerInstitutionTools(server: McpServer, ctx: ToolContext): void {
  registerTool(server, ctx, {
    name: "institution_info",
    title: "Institution info",
    description:
      "College metadata, the server clock in the college timezone, and enabled feature flags. " +
      "Use the server clock instead of the local clock when reasoning about 'now'.",
    shape: {},
    async handler(_args, { client }) {
      const [config, clock, featuresResult] = await Promise.allSettled([
        client.institution.collegeConfig(),
        client.institution.collegeTime(),
        client.institution.features(),
      ]);

      const featureWarnings: string[] = [];
      let features: Array<{ feature: string; enabled: boolean; name: string }> = [];
      if (featuresResult.status === "fulfilled") {
        features = (featuresResult.value.res ?? []).map((f) => ({
          feature: f.feature,
          enabled: f.enabled,
          name: f.featureCustomName || f.defaultName,
        }));
      } else {
        featureWarnings.push(describeError(featuresResult.reason));
      }

      return {
        college:
          config.status === "fulfilled"
            ? {
                name: config.value.name,
                timezone: config.value.timezone,
                currency: config.value.currency,
              }
            : { error: describeError(config.reason) },
        serverTime:
          clock.status === "fulfilled"
            ? { timeZone: clock.value.timeZone, currentTime: clock.value.currentTime }
            : { error: describeError(clock.reason) },
        features,
        ...(featureWarnings.length ? { warnings: featureWarnings } : {}),
      };
    },
  });
}
