import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(fs.readFileSync(path.join(rootDir, "package.json"), "utf8"));
const releaseDir = path.join(rootDir, "frontend", "src-tauri", "target", "release");
const portableDir = path.join(releaseDir, "bundle", "portable");
const sourceExe = path.join(releaseDir, "ag-swarmer-desktop.exe");
const portableExe = path.join(portableDir, `AG Swarmer_${pkg.version}_x64-portable.exe`);

if (!fs.existsSync(sourceExe)) {
  throw new Error(`Missing desktop build artifact: ${sourceExe}`);
}

fs.mkdirSync(portableDir, { recursive: true });
for (const entry of fs.readdirSync(portableDir)) {
  fs.rmSync(path.join(portableDir, entry), { recursive: true, force: true });
}

fs.copyFileSync(sourceExe, portableExe);

console.log(`Portable exe: ${portableExe}`);
