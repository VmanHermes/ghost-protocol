import type { SavedHost } from "./types";

const STORAGE_KEY = "ghost-protocol.hosts";
const LEGACY_KEY = "ghost-protocol.baseUrl";

export function loadHosts(): SavedHost[] {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored) {
    try {
      return JSON.parse(stored) as SavedHost[];
    } catch {
      return [];
    }
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
