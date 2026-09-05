param(
    [ValidateSet('build', 'open', 'dev')][string]$Action = 'build',
    [ValidateSet('aarch64', 'x86_64')][string]$Target = 'aarch64',
    [switch]$Release
)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent

# Use existing Android Studio installations; do not change machine/user settings.
if (!$env:JAVA_HOME) {
    $studio = Get-ItemProperty 'HKLM:/SOFTWARE/Microsoft/Windows/CurrentVersion/Uninstall/*', 'HKCU:/SOFTWARE/Microsoft/Windows/CurrentVersion/Uninstall/*' -ErrorAction SilentlyContinue |
        Where-Object DisplayName -eq 'Android Studio' | Select-Object -First 1
    if ($studio.UninstallString) { $env:JAVA_HOME = Join-Path (Split-Path $studio.UninstallString.Trim('"')) 'jbr' }
}
if (!$env:ANDROID_HOME) { $env:ANDROID_HOME = $env:ANDROID_SDK_ROOT }
if (!$env:ANDROID_HOME) {
    $tables = Get-ChildItem (Join-Path $env:APPDATA 'Google/AndroidStudio*/options/jdk.table.xml') -ErrorAction SilentlyContinue
    foreach ($table in $tables) {
        [xml]$config = Get-Content -LiteralPath $table.FullName -Raw
        $sdk = $config.SelectSingleNode("//jdk[type/@value='Android SDK']/homePath")
        if ($sdk) { $env:ANDROID_HOME = $sdk.value; break }
    }
}
if (!$env:ANDROID_HOME) { $env:ANDROID_HOME = Join-Path $env:LOCALAPPDATA 'Android/Sdk' }
if (!$env:NDK_HOME -and (Test-Path (Join-Path $env:ANDROID_HOME 'ndk'))) {
    $ndk = Get-ChildItem (Join-Path $env:ANDROID_HOME 'ndk') -Directory | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
    if ($ndk) { $env:NDK_HOME = $ndk.FullName }
}
foreach ($entry in @{ JAVA_HOME = $env:JAVA_HOME; ANDROID_HOME = $env:ANDROID_HOME; NDK_HOME = $env:NDK_HOME }.GetEnumerator()) {
    if (!$entry.Value -or !(Test-Path $entry.Value)) { throw "Set $($entry.Key) to your installed toolchain directory." }
}
$env:PATH = "$(Join-Path $env:JAVA_HOME 'bin');$(Join-Path $env:ANDROID_HOME 'platform-tools');$env:PATH"
# Java does not automatically use Windows' active HTTP proxy.
$proxy = Get-ItemProperty 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Internet Settings' -ErrorAction SilentlyContinue
if ($proxy.ProxyEnable -eq 1 -and $proxy.ProxyServer -match '^([^:;=]+):(\d+)$') {
    $env:GRADLE_OPTS = "$env:GRADLE_OPTS -Dhttp.proxyHost=$($Matches[1]) -Dhttp.proxyPort=$($Matches[2]) -Dhttps.proxyHost=$($Matches[1]) -Dhttps.proxyPort=$($Matches[2])"
}
$tauriCli = Join-Path $workspace 'frontend/node_modules/@tauri-apps/cli/tauri.js'
if (!(Test-Path $tauriCli)) { throw 'Run pnpm install first.' }
Push-Location (Join-Path $workspace 'android')
try {
    if ($Action -eq 'open') {
        & node $tauriCli android open
    } elseif ($Action -eq 'dev') {
        & node $tauriCli android dev
    } else {
        $buildArgs = @('android', 'build', '--target', $Target, '--apk')
        if (!$Release) { $buildArgs += '--debug' }
        $ErrorActionPreference = 'Continue'
        & node $tauriCli @buildArgs 2>&1 | Tee-Object -Variable tauriOutput
        $tauriExit = $LASTEXITCODE
        $ErrorActionPreference = 'Stop'
        if ($tauriExit -ne 0 -and ($tauriOutput -join "`n") -match 'Failed to create a symbolic link') {
            # Tauri's staging step uses symlinks, which Windows may deny without
            # Developer Mode. Cargo already succeeded; stage that exact library
            # by copying and let Gradle package it without rerunning the Rust task.
            $triple = if ($Target -eq 'aarch64') { 'aarch64-linux-android' } else { 'x86_64-linux-android' }
            $abi = if ($Target -eq 'aarch64') { 'arm64-v8a' } else { 'x86_64' }
            $arch = if ($Target -eq 'aarch64') { 'Arm64' } else { 'X86_64' }
            $profile = if ($Release) { 'release' } else { 'debug' }
            $variant = if ($Release) { 'Release' } else { 'Debug' }
            $library = "src-tauri/target/$triple/$profile/libqunica_android_lib.so"
            if (!(Test-Path $library)) { throw 'Compiled Android library was not found.' }
            $destination = "src-tauri/gen/android/app/src/main/jniLibs/$abi"
            New-Item -ItemType Directory -Path $destination -Force | Out-Null
            Copy-Item -LiteralPath $library -Destination "$destination/libqunica_android_lib.so" -Force
            $strip = Join-Path $env:NDK_HOME 'toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe'
            & $strip --strip-debug "$destination/libqunica_android_lib.so"
            if ($LASTEXITCODE -ne 0) { throw 'Unable to strip packaged native debug symbols' }
            Push-Location 'src-tauri/gen/android'
            try {
                $localGradle = Join-Path $workspace '.qunica/gradle-8.14.3/bin/gradle.bat'
                $gradle = if (Test-Path $localGradle) { $localGradle } else { './gradlew.bat' }
                & $gradle ":app:assemble$arch$variant" -x ":app:rustBuild$arch$variant"
            } finally { Pop-Location }
        } elseif ($tauriExit -ne 0) { throw "Tauri build failed with exit code $tauriExit" }
    }
    if ($LASTEXITCODE -ne 0) { throw "Android $Action failed with exit code $LASTEXITCODE" }
} finally { Pop-Location }
