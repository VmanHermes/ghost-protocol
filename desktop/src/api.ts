export function wsUrlFromHttp(baseUrl: string) {
  if (baseUrl.startsWith("https://")) return baseUrl.replace("https://", "wss://") + "/ws";
  if (baseUrl.startsWith("http://")) return baseUrl.replace("http://", "ws://") + "/ws";
  return `ws://${baseUrl}/ws`;
}

export function fmt(ts?: string | null) {
  if (!ts) return "—";
  try {
    return new Date(ts).toLocaleString();
  } catch {
    return ts;
  }
}

export async function api<T>(baseUrl: string, path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${baseUrl}${path}`, {
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
    ...init,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `Request failed: ${res.status}`);
  }
  return res.json() as Promise<T>;
}

export type ApiHost = {
  id: string;
  name: string;
  tailscaleIp: string;
  url: string;
  status: string;
  lastSeen: string | null;
  capabilities: {
    gpu: string | null;
    ramGb: number | null;
    hermes: boolean;
    ollama: boolean;
  } | null;
};

export async function listHosts(daemonUrl: string): Promise<ApiHost[]> {
  return api<ApiHost[]>(daemonUrl, "/api/hosts");
}

export async function addHostApi(
  daemonUrl: string,
  name: string,
  tailscaleIp: string,
): Promise<ApiHost> {
  return api<ApiHost>(daemonUrl, "/api/hosts", {
    method: "POST",
    body: JSON.stringify({ name, tailscaleIp }),
  });
}

export async function removeHostApi(
  daemonUrl: string,
  hostId: string,
): Promise<void> {
  await fetch(`${daemonUrl}/api/hosts/${hostId}`, { method: "DELETE" });
}
