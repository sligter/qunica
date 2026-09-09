# Android CI and releases

`.github/workflows/android.yml` is shared by normal CI and the release workflow.
It builds ARM64 and x86_64 APKs, runs Kotlin unit tests, verifies APK signatures
and 16 KiB ZIP alignment, and uploads the APKs with SHA-256 checksums.
Normal CI uses debug signing without repository secrets. Releases use the
release profile and publish only after all Android, desktop and server builds pass.

Toolchain: Java 17, Android API 36, Build Tools 36.0.0, NDK 28.2.13676358,
the checked-in Gradle wrapper, Rust stable and the locked pnpm dependencies.
The existing native project is built directly; do not run `tauri android init`
over the custom Kotlin plugins and manifest.

Repository Actions secrets:

- `ANDROID_KEY_BASE64`: base64-encoded release keystore.
- `ANDROID_KEY_ALIAS`: signing key alias.
- `ANDROID_KEY_PASSWORD`: key and keystore password.

Gradle reads `ANDROID_KEYSTORE_PATH`, `ANDROID_KEY_ALIAS` and
`ANDROID_KEY_PASSWORD` for release signing. CI restores the keystore to its
temporary directory and removes it after the build. Keep a private backup of
the original keystore and password; future APK upgrades must use the same key.
Never commit signing keys or passwords. See the
[Tauri signing guide](https://v2.tauri.app/distribute/sign/android/).

The first official APK uses a different certificate from local debug APKs.
Installing it over a debug APK requires uninstalling that build and pairing
again. Device camera, pairing, and lifecycle behavior still require real-device
acceptance testing; CI unit tests do not replace it.
