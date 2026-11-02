# macOS application signing

Qunica's macOS bundles use Tauri's `bundle.macOS.signingIdentity: "-"`
to ad-hoc sign the **complete application before packaging**. This seals its
resources as well as its executable, without a paid Apple Developer account.
Both Intel and Apple Silicon builds use this configuration.

This fixes the missing application resource signature reported in
[issue #5](https://github.com/sligter/qunica/issues/5). The v0.1.8 application
archives lack `Contents/_CodeSignature/CodeResources`; the ARM64 executable has
only a linker-generated ad-hoc signature. The v0.1.8 download audit verified
Tauri updater signatures, not macOS application signatures. Existing v0.1.8
assets do not change when this source configuration changes; a newly built
release is required to distribute the fix.

## Signing boundaries

- **Application signature:** macOS `codesign` checks the executable and sealed
  resources. An executable's linker signature alone does not sign an app bundle.
- **Updater signature:** the separate `.sig` file authenticates the downloaded
  archive using Qunica's updater public key. It does not provide an Apple
  developer identity or make the application pass Gatekeeper.
- **Developer ID and notarization:** not configured by this change. Ad-hoc signing
  has no verified developer identity. First launch can still require approval in
  macOS Privacy & Security. It must not be advertised as Apple-notarized.

See [Tauri's signing documentation](https://v2.tauri.app/distribute/sign/macos/)
and [Apple TN2206](https://developer.apple.com/library/archive/technotes/tn2206/_index.html).

## Packaging gate

Both macOS release matrix targets run this check after the Tauri build, in
publishing and dry-run modes:

```sh
node scripts/check-macos-bundle.mjs frontend/src-tauri/target/aarch64-apple-darwin/release/bundle
```

The check requires exactly one updater `.app.tar.gz` and one `.dmg`. It checks
the original `.app`, extracts the updater archive, and mounts the DMG read-only.
Each application must contain a nonempty resource seal and pass:

```sh
codesign --verify --deep --strict --all-architectures --verbose=2 Qunica.app
```

The executable, resource seal and Info.plist must match the original signed
application in both packaged copies. Signature details are logged, but the
check never re-signs or repairs artifacts. Packaging or verification failures
block the desktop job and therefore public release publication. Tauri's action
uploads assets to a draft before this check; successful upload alone is not
acceptance. Gatekeeper assessment (`spctl`) is not a success gate for ad-hoc
releases because no Apple-authenticated identity is provided.

## Regression checks

```sh
node scripts/check-release-workflow.mjs
node --test scripts/test-macos-bundle.mjs
```

The portable tests reject missing resource seals and ambiguous/missing archives.
On macOS, tests compile a minimal app, sign its complete bundle, create an updater
archive and DMG, and verify the packaged copies. They also reject resource
tampering and correctly signed but different applications in either package.
The macOS CI job runs these real checks without certificates or secrets.
Other hosts explicitly skip this integration test; a Windows result is not
evidence of successful macOS signing or installation.

Revisit this decision when Developer ID credentials are available, if additional
executables/frameworks are bundled, or if signature/installation failures recur.
Formal signing needs its own identity, notarization and Gatekeeper acceptance
checks. Archive verification does not replace a downloaded-app first-launch test
on an independent Mac.
