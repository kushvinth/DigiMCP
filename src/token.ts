/**
 * Token provider.
 *
 * The token is re-read from `process.env.DIGII_TOKEN` on **every call**, so
 * rotating it takes effect without rebuilding the SDK client. The SDK accepts a
 * function token provider and therefore re-reads it per request too.
 */
export function tokenProvider(env: NodeJS.ProcessEnv = process.env): string {
  const token = env.DIGII_TOKEN?.trim();
  if (!token) {
    throw new Error(
      "fijii-mcp: DIGII_TOKEN is not set. Provide the `user` cookie from the logged-in portal.",
    );
  }
  return token;
}
