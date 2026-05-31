import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = JSON.parse(fs.readFileSync(path.join(rootDir, "package.json"), "utf8"));
const releaseDir = path.join(rootDir, "frontend", "src-tauri", "target", "release");
const portableDir = path.join(releaseDir, "bundle", "portable");
const appDir = path.join(portableDir, "AG Swarmer");
const zipPath = path.join(portableDir, `AG Swarmer_${pkg.version}_x64-portable.zip`);

const files = [
  {
    from: path.join(releaseDir, "ag-swarmer-desktop.exe"),
    to: path.join(appDir, "AG Swarmer.exe"),
  },
  {
    from: path.join(releaseDir, "ag-swarmer-backend.exe"),
    to: path.join(appDir, "ag-swarmer-backend.exe"),
  },
];

for (const file of files) {
  if (!fs.existsSync(file.from)) {
    throw new Error(`Missing desktop build artifact: ${file.from}`);
  }
}

fs.rmSync(appDir, { recursive: true, force: true });
fs.mkdirSync(appDir, { recursive: true });

for (const file of files) {
  fs.copyFileSync(file.from, file.to);
}

fs.writeFileSync(
  path.join(appDir, "README.txt"),
  [
    "AG Swarmer portable build",
    "",
    "Run AG Swarmer.exe directly. Keep ag-swarmer-backend.exe in the same folder.",
    "The desktop backend stores local data under the Windows app data directory.",
    "",
  ].join("\r\n"),
  "utf8",
);

fs.rmSync(zipPath, { force: true });

function psQuote(value) {
  return `'${value.replace(/'/g, "''")}'`;
}

const command = [
  "Compress-Archive",
  "-LiteralPath",
  psQuote(appDir),
  "-DestinationPath",
  psQuote(zipPath),
  "-Force",
].join(" ");

const result = spawnSync(
  "powershell",
  ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", command],
  { stdio: "inherit" },
);

if (result.status !== 0) {
  throw new Error(`Portable zip packaging failed with exit code ${result.status}`);
}

console.log(`Portable zip: ${zipPath}`);
