/** Runtime configuration, read from the environment. */

export interface Config {
  baseUrl: string;
  timeZone: string;
  timeoutMs: number;
}

const DEFAULT_TIMEZONE = "Asia/Kolkata";
const DEFAULT_TIMEOUT_MS = 40_000;

/**
 * Load configuration from the environment.
 *
 * `DIGII_BASE_URL` is required — the SDK ships no default host and must target
 * the caller's own institution.
 */
export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  const baseUrl = env.DIGII_BASE_URL?.trim();
  if (!baseUrl) {
    throw new Error(
      "fijii-mcp: DIGII_BASE_URL is not set. Set it to your portal origin, " +
        "e.g. https://<your-institute>.digiicampus.com",
    );
  }

  const timeZone = env.DIGII_TIMEZONE?.trim() || DEFAULT_TIMEZONE;

  const rawTimeout = env.DIGII_TIMEOUT_MS?.trim();
  const timeoutMs = rawTimeout ? Number(rawTimeout) : DEFAULT_TIMEOUT_MS;
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error(`fijii-mcp: DIGII_TIMEOUT_MS must be a positive number, got "${rawTimeout}"`);
  }

  return { baseUrl, timeZone, timeoutMs };
}
