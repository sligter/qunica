import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { validateBundle, verifyApp } from "./check-macos-bundle.mjs";

const validator = fileURLToPath(new URL("./check-macos-bundle.mjs", import.meta.url));

test("a bare executable/updater signature cannot substitute for the resource seal", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "qunica-unsigned-app-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const binary = path.join(root, "Qunica.app/Contents/MacOS/qunica-desktop");
  fs.mkdirSync(path.dirname(binary), { recursive: true });
  fs.writeFileSync(binary, "linker-signed executable");
  fs.writeFileSync(path.join(root, "Qunica.app.tar.gz.sig"), "updater signature");
  assert.throws(() => verifyApp(path.join(root, "Qunica.app")), /Missing bundle resource signature/);
});

test("missing or ambiguous update archives fail before verification", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "qunica-ambiguous-app-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, "macos"));
  assert.throws(() => validateBundle(root), /Expected exactly one \.app\.tar\.gz.*found 0/);
  for (const name of ["one.app.tar.gz", "two.app.tar.gz"]) {
    fs.writeFileSync(path.join(root, "macos", name), "archive");
  }
  assert.throws(() => validateBundle(root), /Expected exactly one \.app\.tar\.gz.*found 2/);
});

test("missing or ambiguous DMGs fail before verification", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "qunica-ambiguous-dmg-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, "macos"));
  fs.mkdirSync(path.join(root, "dmg"));
  fs.writeFileSync(path.join(root, "macos/Qunica.app.tar.gz"), "archive");
  assert.throws(() => validateBundle(root), /Expected exactly one \.dmg.*found 0/);
  for (const name of ["one.dmg", "two.dmg"]) {
    fs.writeFileSync(path.join(root, "dmg", name), "image");
  }
  assert.throws(() => validateBundle(root), /Expected exactly one \.dmg.*found 2/);
});

function run(command, args, options = {}) {
  return execFileSync(command, args, { stdio: "pipe", ...options });
}

function sign(app) {
  run("codesign", ["--force", "--sign", "-", app]);
}

function packArchive(bundle, app) {
  run("tar", ["-czf", path.join(bundle, "macos/Qunica.app.tar.gz"), "-C", path.dirname(app), "Qunica.app"]);
}

function packDmg(bundle, staging) {
  run("hdiutil", ["create", "-ov", "-volname", "Qunica test", "-srcfolder", staging,
    "-format", "UDZO", path.join(bundle, "dmg/Qunica.dmg")]);
}

function check(bundle, expectedError) {
  const result = spawnSync(process.execPath, [validator, bundle], { encoding: "utf8", timeout: 120_000 });
  const output = `${result.stdout}\n${result.stderr}`;
  assert.ifError(result.error);
  if (expectedError) {
    assert.notEqual(result.status, 0, output);
    assert.match(output, expectedError);
  } else {
    assert.equal(result.status, 0, output);
  }
}

test("real macOS signatures survive packaging and reject modified artifacts", {
  skip: process.platform !== "darwin" ? "requires macOS codesign and hdiutil" : false,
  timeout: 300_000,
}, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "qunica-signed-bundle-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const pristine = path.join(root, "pristine bundle");
  const app = path.join(pristine, "macos/Qunica.app");
  fs.mkdirSync(path.join(app, "Contents/MacOS"), { recursive: true });
  fs.mkdirSync(path.join(app, "Contents/Resources"));
  fs.mkdirSync(path.join(pristine, "dmg"));
  fs.writeFileSync(path.join(app, "Contents/Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>qunica.signing-test</string>
<key>CFBundleExecutable</key><string>qunica-desktop</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
</dict></plist>
`);
  const resource = "Contents/Resources/example.txt";
  fs.writeFileSync(path.join(app, resource), "original resource");
  run("xcrun", ["clang", "-x", "c", "-", "-o", path.join(app, "Contents/MacOS/qunica-desktop")],
    { input: "int main(void) { return 0; }\n" });
  sign(app);
  packDmg(pristine, path.join(pristine, "macos"));
  packArchive(pristine, app);

  await t.test("signed app, archive and read-only DMG pass, including paths with spaces", () => check(pristine));

  await t.test("a missing resource seal fails even when a binary signature exists", () => {
    const bundle = path.join(root, "missing seal");
    fs.cpSync(pristine, bundle, { recursive: true });
    fs.rmSync(path.join(bundle, "macos/Qunica.app/Contents/_CodeSignature"), { recursive: true });
    check(bundle, /Missing bundle resource signature/);
  });

  await t.test("tampered updater resources are rejected", () => {
    const bundle = path.join(root, "tampered archive");
    fs.cpSync(pristine, bundle, { recursive: true });
    const staging = path.join(root, "archive staging");
    const modified = path.join(staging, "Qunica.app");
    fs.cpSync(app, modified, { recursive: true });
    fs.writeFileSync(path.join(modified, resource), "tampered");
    packArchive(bundle, modified);
    check(bundle, /a sealed resource is missing or invalid|file modified|invalid signature/);
  });

  await t.test("a valid but different signed updater app is rejected", () => {
    const bundle = path.join(root, "different archive");
    fs.cpSync(pristine, bundle, { recursive: true });
    const modified = path.join(root, "different archive staging/Qunica.app");
    fs.cpSync(app, modified, { recursive: true });
    fs.writeFileSync(path.join(modified, resource), "different signed resource");
    sign(modified);
    packArchive(bundle, modified);
    check(bundle, /Updater archive contains a different signed application/);
  });

  await t.test("tampered DMG resources are rejected", () => {
    const bundle = path.join(root, "tampered dmg");
    fs.cpSync(pristine, bundle, { recursive: true });
    const staging = path.join(root, "dmg staging");
    const modified = path.join(staging, "Qunica.app");
    fs.cpSync(app, modified, { recursive: true });
    fs.writeFileSync(path.join(modified, resource), "tampered");
    packDmg(bundle, staging);
    check(bundle, /a sealed resource is missing or invalid|file modified|invalid signature/);
  });

  await t.test("a valid but different signed DMG app is rejected", () => {
    const bundle = path.join(root, "different dmg");
    fs.cpSync(pristine, bundle, { recursive: true });
    const staging = path.join(root, "different dmg staging");
    const modified = path.join(staging, "Qunica.app");
    fs.cpSync(app, modified, { recursive: true });
    fs.writeFileSync(path.join(modified, resource), "different signed resource");
    sign(modified);
    packDmg(bundle, staging);
    check(bundle, /DMG contains a different signed application/);
  });
});
