#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const TARGETS = Object.freeze({
  "x86_64-unknown-linux-gnu": {
    artifact: "ag-swarmer-server-linux-x64.tar.gz",
    archive: "tar.gz",
    extension: ".tar.gz",
    exe: "ag-swarmer-server",
    sourceExe: "ag-swarmer-backend",
  },
  "aarch64-unknown-linux-gnu": {
    artifact: "ag-swarmer-server-linux-arm64.tar.gz",
    archive: "tar.gz",
    extension: ".tar.gz",
    exe: "ag-swarmer-server",
    sourceExe: "ag-swarmer-backend",
  },
  "x86_64-apple-darwin": {
    artifact: "ag-swarmer-server-darwin-x64.tar.gz",
    archive: "tar.gz",
    extension: ".tar.gz",
    exe: "ag-swarmer-server",
    sourceExe: "ag-swarmer-backend",
  },
  "aarch64-apple-darwin": {
    artifact: "ag-swarmer-server-darwin-arm64.tar.gz",
    archive: "tar.gz",
    extension: ".tar.gz",
    exe: "ag-swarmer-server",
    sourceExe: "ag-swarmer-backend",
  },
  "x86_64-pc-windows-msvc": {
    artifact: "ag-swarmer-server-windows-x64.zip",
    archive: "zip",
    extension: ".zip",
    exe: "ag-swarmer-server.exe",
    sourceExe: "ag-swarmer-backend.exe",
  },
});

function main() {
  if (process.argv.includes("--self-test")) {
    selfTest();
    return;
  }

  const target = requireEnv("RELEASE_TARGET");
  const version = requireEnv("RELEASE_VERSION");
  const meta = TARGETS[target];
  if (!meta) {
    fail(`Unsupported RELEASE_TARGET "${target}". Expected one of: ${Object.keys(TARGETS).join(", ")}`);
  }
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
    fail(`RELEASE_VERSION "${version}" is not a semver version without the leading v.`);
  }

  const artifact = process.env.RELEASE_ARTIFACT || meta.artifact;
  if (artifact !== meta.artifact) {
    fail(`RELEASE_ARTIFACT "${artifact}" does not match the expected artifact for ${target}: ${meta.artifact}`);
  }

  const repoRoot = process.cwd();
  const distDir = path.resolve(repoRoot, process.env.RELEASE_DIST_DIR || "dist/release-server");
  const targetDir = path.resolve(repoRoot, process.env.RELEASE_BACKEND_TARGET_DIR || "backend-rs/target");
  const webDir = path.resolve(repoRoot, process.env.RELEASE_WEB_DIR || "frontend/dist");
  const sourceBinary = path.join(targetDir, target, "release", meta.sourceExe);
  const archivePath = path.join(distDir, artifact);
  const workRoot = path.join(distDir, ".work");
  const baseName = stripExtension(artifact, meta.extension);
  const stagingDir = path.join(workRoot, baseName);
  const verifyDir = path.join(workRoot, `${baseName}-verify`);

  assertFile(sourceBinary, `Built backend binary not found for ${target}`);
  assertDirectory(webDir, "Built frontend dist directory not found");
  assertFile(path.join(webDir, "index.html"), "Built frontend dist is missing index.html");

  fs.mkdirSync(workRoot, { recursive: true });
  safeRemove(stagingDir, workRoot);
  safeRemove(verifyDir, workRoot);
  fs.mkdirSync(stagingDir, { recursive: true });

  const packagedBinary = path.join(stagingDir, meta.exe);
  fs.copyFileSync(sourceBinary, packagedBinary);
  if (meta.exe !== "ag-swarmer-server.exe") {
    fs.chmodSync(packagedBinary, 0o755);
  }
  fs.cpSync(webDir, path.join(stagingDir, "web"), { recursive: true });
  smokeCheckDirectory(stagingDir, meta);

  fs.mkdirSync(distDir, { recursive: true });
  if (fs.existsSync(archivePath)) {
    assertInside(archivePath, distDir);
    fs.rmSync(archivePath, { force: true });
  }

  if (meta.archive === "zip") {
    createZip(stagingDir, archivePath);
  } else {
    createTarGz(stagingDir, archivePath);
  }

  assertFile(archivePath, "Archive was not created");
  fs.mkdirSync(verifyDir, { recursive: true });
  extractArchive(archivePath, verifyDir, meta);
  smokeCheckDirectory(verifyDir, meta);

  console.log(JSON.stringify({
    target,
    version,
    artifact,
    archive: path.relative(repoRoot, archivePath).replaceAll(path.sep, "/"),
  }, null, 2));
}

function selfTest() {
  const artifacts = new Set();
  for (const [target, meta] of Object.entries(TARGETS)) {
    if (artifacts.has(meta.artifact)) {
      fail(`Duplicate artifact name: ${meta.artifact}`);
    }
    artifacts.add(meta.artifact);
    if (!meta.artifact.startsWith("ag-swarmer-server-")) {
      fail(`Unexpected artifact prefix for ${target}: ${meta.artifact}`);
    }
    if (!meta.artifact.endsWith(meta.extension)) {
      fail(`Artifact extension mismatch for ${target}: ${meta.artifact}`);
    }
  }
  console.log(`Server release package self-test passed for ${artifacts.size} targets.`);
}

function createTarGz(stagingDir, archivePath) {
  execFileSync("tar", ["-czf", archivePath, "."], {
    cwd: stagingDir,
    stdio: "inherit",
  });
}

function createZip(stagingDir, archivePath) {
  const script = [
    "$ErrorActionPreference = 'Stop'",
    `$items = Get-ChildItem -LiteralPath ${psString(stagingDir)} -Force`,
    "if ($items.Count -eq 0) { throw 'No files to archive' }",
    `Compress-Archive -Path $items.FullName -DestinationPath ${psString(archivePath)} -Force`,
  ].join("; ");
  execFileSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script], {
    stdio: "inherit",
  });
}

function extractArchive(archivePath, verifyDir, meta) {
  if (meta.archive === "zip") {
    const script = [
      "$ErrorActionPreference = 'Stop'",
      `Expand-Archive -LiteralPath ${psString(archivePath)} -DestinationPath ${psString(verifyDir)} -Force`,
    ].join("; ");
    execFileSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script], {
      stdio: "inherit",
    });
    return;
  }
  execFileSync("tar", ["-xzf", archivePath, "-C", verifyDir], {
    stdio: "inherit",
  });
}

function smokeCheckDirectory(dir, meta) {
  assertFile(path.join(dir, meta.exe), `Archive payload is missing ${meta.exe}`);
  assertDirectory(path.join(dir, "web"), "Archive payload is missing web directory");
  assertFile(path.join(dir, "web", "index.html"), "Archive payload web directory is missing index.html");
}

function stripExtension(fileName, extension) {
  if (!fileName.endsWith(extension)) {
    fail(`${fileName} does not end with ${extension}`);
  }
  return fileName.slice(0, -extension.length);
}

function assertFile(filePath, message) {
  if (!fs.existsSync(filePath) || !fs.statSync(filePath).isFile()) {
    fail(`${message}: ${filePath}`);
  }
}

function assertDirectory(dirPath, message) {
  if (!fs.existsSync(dirPath) || !fs.statSync(dirPath).isDirectory()) {
    fail(`${message}: ${dirPath}`);
  }
}

function requireEnv(name) {
  const value = process.env[name];
  if (!value) {
    fail(`${name} is required`);
  }
  return value;
}

function safeRemove(targetPath, rootPath) {
  assertInside(targetPath, rootPath);
  fs.rmSync(targetPath, { recursive: true, force: true });
}

function assertInside(targetPath, rootPath) {
  const resolvedTarget = path.resolve(targetPath);
  const resolvedRoot = path.resolve(rootPath);
  const relative = path.relative(resolvedRoot, resolvedTarget);
  if (relative === "" || relative.startsWith("..") || path.isAbsolute(relative)) {
    fail(`Refusing to operate outside ${resolvedRoot}: ${resolvedTarget}`);
  }
}

function psString(value) {
  return `'${value.replaceAll("'", "''")}'`;
}

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

main();
