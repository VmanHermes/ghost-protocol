import type { SavedHost } from "./types";

const STORAGE_KEY = "ghost-protocol.hosts";
const LEGACY_KEY = "ghost-protocol.baseUrl";

function isValidHost(h: unknown): h is SavedHost {
  return (
    typeof h === "object" && h !== null &&
    typeof (h as SavedHost).id === "string" &&
    typeof (h as SavedHost).name === "string" &&
    typeof (h as SavedHost).url === "string" &&
    ((h as SavedHost).url.startsWith("http://") || (h as SavedHost).url.startsWith("https://"))
  );
}

export function loadHosts(): SavedHost[] {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored) {
    try {
      const parsed: unknown = JSON.parse(stored);
      if (Array.isArray(parsed)) {
        const valid = parsed.filter(isValidHost);
        if (valid.length > 0) return valid;
      }
    } catch {
      // Fall through to legacy migration
    }
    localStorage.removeItem(STORAGE_KEY);
  }

  const legacy = localStorage.getItem(LEGACY_KEY);
  if (legacy) {
    const hosts: SavedHost[] = [{ id: crypto.randomUUID(), name: "Default", url: legacy }];
    localStorage.setItem(STORAGE_KEY, JSON.stringify(hosts));
    localStorage.removeItem(LEGACY_KEY);
    return hosts;
  }

  return [];
}

export function saveHosts(hosts: SavedHost[]): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(hosts));
}

export function addHost(hosts: SavedHost[], name: string, url: string): SavedHost[] {
  const next = [...hosts, { id: crypto.randomUUID(), name, url }];
  saveHosts(next);
  return next;
}

export function removeHost(hosts: SavedHost[], hostId: string): SavedHost[] {
  const next = hosts.filter((h) => h.id !== hostId);
  saveHosts(next);
  return next;
}
