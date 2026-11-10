/**
 * Clean portal strings: the backend sprinkles non-breaking spaces and doubled
 * whitespace into course names and codes.
 */
export function normaliseText(value: string | null | undefined): string {
  return (value ?? "").replace(/\u00a0/g, " ").replace(/\s+/g, " ").trim();
}

/** Format a Date as an ISO string, or null. */
export function isoOrNull(date: Date | null | undefined): string | null {
  return date ? date.toISOString() : null;
}
