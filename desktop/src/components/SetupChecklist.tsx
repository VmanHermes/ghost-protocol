import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type CheckStatus = "pending" | "ok" | "missing" | "too_old";

type CheckItem = {
  name: string;
  key: string;
  status: CheckStatus;
  version: string | null;
  minVersion: string;
};

type Props = {
  visible: boolean;
  onDismiss: () => void;
  onHostDetected: (name: string, url: string) => void;
};

const INITIAL_CHECKS: CheckItem[] = [
  { name: "Python", key: "python", status: "pending", version: null, minVersion: "3.10" },
  { name: "tmux", key: "tmux", status: "pending", version: null, minVersion: "3.0" },
  { name: "Tailscale", key: "tailscale", status: "pending", version: null, minVersion: "1.0" },
  { name: "Tailscale mesh", key: "tailscale_mesh", status: "pending", version: null, minVersion: "" },
  { name: "Daemon", key: "daemon", status: "pending", version: null, minVersion: "" },
];

type InstallCommands = Record<string, Record<string, string>>;

function getInstallCommands(packageManager: string): InstallCommands {
  const pm = packageManager;
  return {
    python: {
      apt: "sudo apt install python3",
      dnf: "sudo dnf install python3",
      pacman: "sudo pacman -S python",
      brew: "brew install python3",
      unknown: "Install Python 3.10+ using your system package manager",
    },
    tmux: {
      apt: "sudo apt install tmux",
      dnf: "sudo dnf install tmux",
      pacman: "sudo pacman -S tmux",
      brew: "brew install tmux",
      unknown: "Install tmux 3.0+ using your system package manager",
    },
    tailscale: {
      apt: "curl -fsSL https://tailscale.com/install.sh | sh",
      dnf: "curl -fsSL https://tailscale.com/install.sh | sh",
      pacman: "sudo pacman -S tailscale",
      brew: "brew install tailscale",
      unknown: "curl -fsSL https://tailscale.com/install.sh | sh",
    },
    tailscale_mesh: {
      [pm]: "sudo systemctl enable --now tailscaled && sudo tailscale up",
      unknown: "sudo systemctl enable --now tailscaled && sudo tailscale up",
    },
    daemon: {
      [pm]: "pip install ghost-protocol-daemon && ghost-protocol-daemon",
      unknown: "pip install ghost-protocol-daemon && ghost-protocol-daemon",
    },
  };
}

function getCommand(commands: InstallCommands, key: string, pm: string): string {
  const group = commands[key];
  if (!group) return "";
  return group[pm] ?? group["unknown"] ?? "";
}

const DOT_COLORS: Record<CheckStatus, string> = {
  pending: "#8c95a4",
  ok: "#10b981",
  missing: "#ef4444",
  too_old: "#ef4444",
};

export function SetupChecklist({ visible, onDismiss, onHostDetected }: Props) {
  const [checks, setChecks] = useState<CheckItem[]>(INITIAL_CHECKS);
  const [packageManager, setPackageManager] = useState("unknown");
  const [allDone, setAllDone] = useState(false);
  const hostDetectedRef = useRef(false);

  const runDetection = useCallback(async () => {
    try {
      const pm = await invoke<string>("detect_package_manager").catch(() => "unknown");
      setPackageManager(typeof pm === "string" ? pm : "unknown");
    } catch {
      // ignore
    }

    const results: CheckItem[] = [...INITIAL_CHECKS];

    const [pythonResult, tmuxResult, tailscaleResult, tailscaleMeshResult, daemonResult] = await Promise.allSettled([
      invoke<string>("detect_python"),
      invoke<string>("detect_tmux"),
      invoke<string>("detect_tailscale"),
      invoke<string>("detect_tailscale_ip"),
      invoke<string>("detect_daemon"),
    ]);

    const processResult = (
      index: number,
      result: PromiseSettledResult<string>,
    ) => {
      if (result.status === "fulfilled") {
        results[index] = { ...results[index], status: "ok", version: result.value };
      } else {
        const reason = String(result.reason ?? "");
        if (reason.includes("version_too_old")) {
          const ver = reason.split("version_too_old:")[1] ?? null;
          results[index] = { ...results[index], status: "too_old", version: ver };
        } else {
          results[index] = { ...results[index], status: "missing", version: null };
        }
      }
    };

    processResult(0, pythonResult);
    processResult(1, tmuxResult);
    processResult(2, tailscaleResult);
    processResult(3, tailscaleMeshResult);
    processResult(4, daemonResult);

    setChecks(results);

    if (results[4].status === "ok" && !hostDetectedRef.current) {
      hostDetectedRef.current = true;
      onHostDetected("This Computer", "http://127.0.0.1:8787");
    }

    if (results.every((c) => c.status === "ok")) {
      setAllDone(true);
    }
  }, [onHostDetected]);

  useEffect(() => {
    if (!visible) return;
    void runDetection();
    const interval = setInterval(() => void runDetection(), 3000);
    return () => clearInterval(interval);
  }, [visible, runDetection]);

  useEffect(() => {
    if (!allDone) return;
    const timer = setTimeout(onDismiss, 2000);
    return () => clearTimeout(timer);
  }, [allDone, onDismiss]);

  if (!visible) return null;

  const installCommands = getInstallCommands(packageManager);
  const activeItem = allDone ? null : checks.find((c) => c.status === "missing" || c.status === "too_old") ?? null;

  const handleCopy = (text: string) => {
    void navigator.clipboard.writeText(text);
  };

  return (
    <div className="setup-checklist">
      <div className="setup-checklist-items">
        {checks.map((item) => (
          <div key={item.key} className="setup-checklist-item">
            <span
              className="setup-checklist-dot"
              style={{ background: DOT_COLORS[item.status] }}
            />
            <span className="setup-checklist-label">
              {item.name}
              {item.status === "ok" && item.version ? ` ${item.version}` : ""}
            </span>
          </div>
        ))}
      </div>

      {allDone && (
        <div className="setup-checklist-message">All set!</div>
      )}

      {activeItem && !allDone && (
        <div className="setup-checklist-detail">
          <div className="setup-checklist-message">
            {activeItem.status === "too_old"
              ? `${activeItem.name} ${activeItem.version} is installed but version ${activeItem.minVersion}+ is required.`
              : activeItem.key === "tailscale_mesh"
                ? "Tailscale is installed but not connected to a mesh. Start the service, then log in via the browser link."
                : `${activeItem.name} is not installed.`}
          </div>
          {(() => {
            const cmd = getCommand(installCommands, activeItem.key, packageManager);
            return cmd ? (
              <div className="setup-checklist-command">
                <code>{cmd}</code>
                <button
                  className="setup-checklist-copy"
                  onClick={() => handleCopy(cmd)}
                  title="Copy command"
                >
                  Copy
                </button>
              </div>
            ) : null;
          })()}
        </div>
      )}

      <button className="setup-checklist-dismiss" onClick={onDismiss}>
        Dismiss
      </button>
    </div>
  );
}
