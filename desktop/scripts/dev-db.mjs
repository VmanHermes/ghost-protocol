import { mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..", "..");
const devDbPath = path.join(repoRoot, "data", "dev", "ghost_protocol-dev.db");

async function main() {
  const command = process.argv[2];

  if (command === "path") {
    console.log(devDbPath);
    return;
  }

  if (command === "reset") {
    await mkdir(path.dirname(devDbPath), { recursive: true });
    await Promise.all(
      ["", "-wal", "-shm"].map((suffix) =>
        rm(`${devDbPath}${suffix}`, { force: true }),
      ),
    );
    console.log(`Reset dev database at ${devDbPath}`);
    return;
  }

  console.error("Usage: node ./scripts/dev-db.mjs <path|reset>");
  process.exitCode = 1;
}

await main();
