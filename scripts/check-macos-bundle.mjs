#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

function run(command, args) {
  execFileSync(command, args, { stdio: "inherit" });
}

function digest(file) {
  assert.ok(fs.statSync(file).size > 0, `Empty bundle file: ${file}`);
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

export function verifyApp(app) {
  const plist = path.join(app, "Contents/Info.plist");
  const executable = path.join(app, "Contents/MacOS/qunica-desktop");
  const resources = path.join(app, "Contents/_CodeSignature/CodeResources");
  // Note: A linker signature or updater .sig does not seal the app's resources.
  // See specs/macos-signing.md for the distinction from Gatekeeper acceptance.
  assert.ok(fs.existsSync(resources), `Missing bundle resource signature: ${resources}`);
  const fingerprint = [plist, executable, resources].map(digest).join(":");
  run("codesign", ["--verify", "--deep", "--strict", "--all-architectures", "--verbose=2", app]);
  run("codesign", ["--display", "--verbose=4", app]);
  return fingerprint;
}

function oneFile(directory, suffix) {
  const matches = fs.readdirSync(directory)
    .filter(name => name.endsWith(suffix) && fs.statSync(path.join(directory, name)).isFile());
  assert.equal(matches.length, 1, `Expected exactly one ${suffix} in ${directory}; found ${matches.length}`);
  return path.join(directory, matches[0]);
}

export function validateBundle(directory) {
  const bundle = path.resolve(directory);
  const archive = oneFile(path.join(bundle, "macos"), ".app.tar.gz");
  const dmg = oneFile(path.join(bundle, "dmg"), ".dmg");
  const original = verifyApp(path.join(bundle, "macos/Qunica.app"));
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "qunica-macos-signature-"));
  const extracted = path.join(temporary, "archive");
  const mount = path.join(temporary, "mount");
  let mounted = false;
  try {
    fs.mkdirSync(extracted);
    run("tar", ["-xzf", archive, "-C", extracted]);
    assert.equal(verifyApp(path.join(extracted, "Qunica.app")), original,
      "Updater archive contains a different signed application");

    fs.mkdirSync(mount);
    run("hdiutil", ["attach", dmg, "-readonly", "-nobrowse", "-mountpoint", mount]);
    mounted = true;
    assert.equal(verifyApp(path.join(mount, "Qunica.app")), original,
      "DMG contains a different signed application");
  } finally {
    // Note: If detach fails, leave the temporary directory intact; never remove a mounted image.
    if (mounted) run("hdiutil", ["detach", mount]);
    fs.rmSync(temporary, { recursive: true, force: true });
  }
  console.log("macOS app, updater archive and DMG signatures verified (not Apple notarization).");
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  assert.equal(process.platform, "darwin", "Bundle signature verification requires macOS");
  assert.equal(process.argv.length, 3, "Usage: node scripts/check-macos-bundle.mjs <bundle-directory>");
  validateBundle(process.argv[2]);
}
