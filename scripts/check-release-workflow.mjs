#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const WORKFLOW = ".github/workflows/release.yml";
const TAURI_CONFIG = "frontend/src-tauri/tauri.conf.json";
const TAURI_CARGO = "frontend/src-tauri/Cargo.toml";

const DESKTOP_TARGETS = [
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
];

const SERVER_TARGETS = [
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
  "x86_64-pc-windows-msvc",
];

const SERVER_ARTIFACTS = [
  "ag-swarmer-server-linux-x64.tar.gz",
  "ag-swarmer-server-linux-arm64.tar.gz",
  "ag-swarmer-server-darwin-x64.tar.gz",
  "ag-swarmer-server-darwin-arm64.tar.gz",
  "ag-swarmer-server-windows-x64.zip",
];

const failures = [];
const notes = [];

function main() {
  const workflowText = readText(WORKFLOW);
  const requireUpdater = isTruthy(process.env.RELEASE_REQUIRE_UPDATER);
  checkWorkflowShape(workflowText);
  checkUpdaterState(workflowText, requireUpdater);
  checkMacOsIcon();

  if (failures.length > 0) {
    console.error("Release workflow validation failed:");
    for (const failure of failures) {
      console.error(`- ${failure}`);
    }
    process.exit(1);
  }

  console.log("Release workflow validation passed.");
  for (const note of notes) {
    console.log(note);
  }
}

function checkMacOsIcon() {
  const tauriConfig = JSON.parse(readText(TAURI_CONFIG));
  const icons = tauriConfig?.bundle?.icon || [];
  const hasIcnsIcon = Array.isArray(icons) && icons.includes("icons/icon.icns");
  expect(hasIcnsIcon, "Tauri bundle icon list must include icons/icon.icns for macOS desktop bundles");
  if (hasIcnsIcon) {
    const iconPath = path.join(path.dirname(TAURI_CONFIG), "icons/icon.icns");
    expect(fs.existsSync(iconPath), "frontend/src-tauri/icons/icon.icns must exist for macOS desktop bundles");
  }
}

function checkWorkflowShape(text) {
  expect(!/\t/.test(text), "workflow must not contain tab indentation");
  expect(/name:\s*Release/.test(text), "workflow name should be Release");
  expect(/push:\s*\n\s+tags:\s*\n\s+- ['"]v\*\.\*\.\*['"]/.test(text), "workflow must trigger on v*.*.* tags");
  expect(/workflow_dispatch:/.test(text), "workflow must support workflow_dispatch");

  for (const job of ["validate-release", "create-draft-release", "desktop", "server", "publish-release"]) {
    expect(new RegExp(`^  ${escapeRegExp(job)}:`, "m").test(text), `missing ${job} job`);
  }

  for (const target of DESKTOP_TARGETS) {
    expect(text.includes(target), `missing desktop target ${target}`);
  }
  for (const target of SERVER_TARGETS) {
    expect(text.includes(target), `missing server target ${target}`);
  }
  for (const artifact of SERVER_ARTIFACTS) {
    expect(text.includes(artifact), `missing server artifact ${artifact}`);
  }

  expect(text.includes("tauri-apps/tauri-action@v1"), "desktop job must use tauri-apps/tauri-action@v1");
  expect(text.includes("releaseDraft: true"), "tauri-action must upload to a draft release");
  expect(text.includes("uploadUpdaterJson: true"), "tauri-action must be ready to upload latest.json");
  expect(text.includes("uploadUpdaterSignatures: true"), "tauri-action must upload updater signatures");
  expect(text.includes("updaterJsonPreferNsis: true"), "Windows updater JSON should prefer NSIS installers");
  expect(text.includes("TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}"), "workflow must reference TAURI_SIGNING_PRIVATE_KEY secret");
  expect(text.includes("TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}"), "workflow must reference TAURI_SIGNING_PRIVATE_KEY_PASSWORD secret");
  expect(text.includes('pnpm --dir frontend exec tauri signer sign "../${RELEASE_DIST_DIR}/${RELEASE_ARTIFACT}"'), "server archives must be signed with the Tauri signer from the frontend working directory");
  expect(!text.includes('> "${RELEASE_DIST_DIR}/${RELEASE_ARTIFACT}.sig"'), "server signing must not redirect signer stdout into .sig files");
  expect(text.includes('test -s "${RELEASE_DIST_DIR}/${RELEASE_ARTIFACT}.sig"'), "server signing must verify the signer-created .sig file exists");
  expect(text.includes("sha256"), "server archives must receive sha256 files");
  expect(text.includes('gh api -X POST "repos/${GITHUB_REPOSITORY}/releases"') && text.includes("-F draft=true"), "workflow must create a draft release before uploads");
  expect(text.includes("releases/assets/${asset_id}"), "workflow must clear stale draft release assets before fresh uploads");
  expect(text.includes("draft=false"), "publish job must clear the draft flag");
  expect(text.includes("merge-base --is-ancestor"), "validate job must enforce default-branch reachability");
  expect(text.includes("tauri.conf.json"), "validate job must compare tag against app config version");
  expect(text.includes("frontend/src-tauri/Cargo.toml"), "validate job must compare tag against Tauri crate version");
  expect(text.includes("backend-rs/Cargo.toml"), "validate job must compare tag against backend workspace version");

  const desktopJob = jobBlock(text, "desktop");
  const serverJob = jobBlock(text, "server");
  const createDraftReleaseJob = jobBlock(text, "create-draft-release");
  const publishJob = jobBlock(text, "publish-release");
  expect(createDraftReleaseJob.includes("actions/checkout@v4"), "create draft release job must checkout before using gh release create");
  expect(createDraftReleaseJob.includes("ref: ${{ needs.validate-release.outputs.tag_commit }}"), "create draft release job must checkout the validated tag commit");
  expect(text.includes('if [[ "${tag}" == *-* ]]; then'), "semver prerelease tags such as alpha and beta must publish as prereleases");
  expect(desktopJob.includes("needs: [validate-release, create-draft-release]"), "desktop job must wait for validation and draft creation");
  expect(serverJob.includes("needs: [validate-release, create-draft-release]"), "server job must wait for validation and draft creation");
  expect(desktopJob.includes("ref: ${{ needs.validate-release.outputs.tag_commit }}"), "desktop job must build the validated tag commit");
  expect(serverJob.includes("ref: ${{ needs.validate-release.outputs.tag_commit }}"), "server job must build the validated tag commit");
  expect(serverJob.includes("shell: bash"), "server job must use bash so Unix-style commands also work on Windows runners");
  expect(desktopJob.includes("ports.ubuntu.com/ubuntu-ports"), "Linux desktop arm64 build must add Ubuntu ports arm64 apt sources");
  expect(serverJob.includes("ports.ubuntu.com/ubuntu-ports"), "Linux server arm64 build must add Ubuntu ports arm64 apt sources");
  expect(desktopJob.includes("PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig"), "Linux desktop arm64 build must set cross PKG_CONFIG_LIBDIR");
  expect(serverJob.includes("PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig"), "Linux server arm64 build must set cross PKG_CONFIG_LIBDIR");
  expect(desktopJob.includes("libssl-dev:arm64"), "Linux desktop arm64 build must install arm64 OpenSSL development files");
  expect(serverJob.includes("libssl-dev:arm64"), "Linux server arm64 build must install arm64 OpenSSL development files");
  expect(publishJob.includes("needs: [validate-release, create-draft-release, desktop, server]"), "publish job must depend on desktop and server jobs");
  expect(publishJob.includes("needs.desktop.result == 'success'"), "publish job must explicitly require successful desktop builds");
  expect(publishJob.includes("needs.server.result == 'success'"), "publish job must explicitly require successful server builds");
}

function checkUpdaterState(text, requireUpdater) {
  const tauriConfig = JSON.parse(readText(TAURI_CONFIG));
  const cargoToml = readText(TAURI_CARGO);
  const createsUpdaterArtifacts = tauriConfig?.bundle?.createUpdaterArtifacts === true;
  const hasUpdaterConfig = Boolean(tauriConfig?.plugins?.updater);
  const hasUpdaterPlugin = /\btauri-plugin-updater\b/.test(cargoToml);

  if (!createsUpdaterArtifacts || !hasUpdaterConfig || !hasUpdaterPlugin) {
    expect(text.includes("UPDATER_LIMITATION"), "workflow must document the current updater/latest.json limitation");
    const message = "Tauri updater artifacts/plugin/config are not fully present, so tauri-action is configured for latest.json but current builds may not emit latest.json or desktop .sig assets.";
    if (requireUpdater) {
      failures.push(`${message} Real release publishing is blocked until bundle.createUpdaterArtifacts, tauri-plugin-updater, and plugins.updater are configured.`);
    } else {
      notes.push(`Note: ${message}`);
    }
    if (process.env.GITHUB_ACTIONS === "true") {
      console.log("::notice title=Updater support::Tauri updater artifacts/plugin/config are not fully present; latest.json upload is configured but may not be emitted until app config is updated.");
    }
    return;
  }

  notes.push("Updater plugin/config detected; tauri-action latest.json upload is expected.");
}

function jobBlock(text, jobName) {
  const start = text.search(new RegExp(`^  ${escapeRegExp(jobName)}:`, "m"));
  if (start === -1) {
    return "";
  }
  const rest = text.slice(start + 1);
  const next = rest.search(/\n  [A-Za-z0-9_-]+:/);
  return next === -1 ? text.slice(start) : text.slice(start, start + 1 + next);
}

function readText(file) {
  const fullPath = path.resolve(process.cwd(), file);
  if (!fs.existsSync(fullPath)) {
    fail(`${file} does not exist`);
  }
  return fs.readFileSync(fullPath, "utf8");
}

function expect(condition, message) {
  if (!condition) {
    failures.push(message);
  }
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function isTruthy(value) {
  return /^(1|true|yes)$/i.test(value || "");
}

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

main();
