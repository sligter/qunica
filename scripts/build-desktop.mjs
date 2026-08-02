import { spawnSync } from "node:child_process";

const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
const args = ["--filter", "@ag-swarmer/frontend", "tauri", "build"];

if (!process.env.TAURI_SIGNING_PRIVATE_KEY && !process.env.TAURI_SIGNING_PRIVATE_KEY_PATH) {
  args.push("--no-sign");
  console.log("No Tauri signing key found; building unsigned bundles.");
}

const result = spawnSync(pnpm, args, {
  shell: process.platform === "win32",
  stdio: "inherit",
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
