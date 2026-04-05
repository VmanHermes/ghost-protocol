/**
 * Returns true when running inside the Tauri desktop app.
 * In a browser/PWA context this returns false.
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
