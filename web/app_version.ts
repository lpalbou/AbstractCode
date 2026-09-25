/** The version the About dialog shows: web/package.json "version". A missing
 * or empty version fails the build (and the tests) instead of shipping the
 * string "undefined". */
export function appVersionFrom(packageJsonText: string): string {
  const version = (JSON.parse(packageJsonText) as { version?: unknown }).version;
  if (typeof version !== "string" || !version.trim())
    throw new Error('web/package.json has no "version": the About dialog needs it; refusing to build.');
  return version.trim();
}
