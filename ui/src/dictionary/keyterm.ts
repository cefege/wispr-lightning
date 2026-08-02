/**
 * Deepgram keyterm validation.
 *
 * Vocabulary phrases are forwarded to Deepgram as `keyterm` parameters.
 * Deepgram accepts a malformed keyterm with HTTP 200 and silently boosts
 * nothing, so a user who types
 * `acme, inc` would otherwise get no error, no warning, and no effect — the
 * word would simply never be recognised any better and they would have no way
 * to find out why.
 *
 * Three constructs break a keyterm, all of them because of how the parameter
 * is tokenised:
 *
 * - a comma, which separates keyterms;
 * - a semicolon, likewise;
 * - a trailing `:<number>`, which is the intensifier suffix, so `plan:9` is
 *   read as the term `plan` boosted by 9 rather than the literal string.
 *
 * This is advisory because a phrase may still be useful as a local
 * replacement or snippet, even when Deepgram cannot use it as a recognition hint.
 */

/** The intensifier suffix, e.g. the `:2` in `deepgram:2`. */
const INTENSIFIER = /:\d+$/;

/**
 * A human-readable reason the phrase will not work as a Deepgram keyterm, or
 * `null` if it will.
 */
export function keytermWarning(phrase: string): string | null {
  const trimmed = phrase.trim();
  if (trimmed === "") return null;

  if (trimmed.includes(",") || trimmed.includes(";")) {
    return "Deepgram splits keyterms on commas and semicolons, so this phrase will not be sent as a recognition hint. A local replacement still applies.";
  }
  if (INTENSIFIER.test(trimmed)) {
    return "Deepgram reads a trailing “:number” as a boost level, so only the text before the colon can be used as a recognition hint. A local replacement still applies.";
  }
  return null;
}
