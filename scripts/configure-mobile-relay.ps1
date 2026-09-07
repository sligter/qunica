param(
    [Parameter(Mandatory = $true)][string]$VpsHost,
    [ValidateRange(1, 65535)][int]$ControlPort = 7000,
    [ValidateRange(1, 65535)][int]$PhonePort = 18766
)
$ErrorActionPreference = 'Stop'
# Generate configuration only. Never install software, open ports, or start services.
if ($ControlPort -eq $PhonePort) { throw 'ControlPort and PhonePort must differ.' }
if ($VpsHost -notmatch '^[a-zA-Z0-9.-]+$' -or [Uri]::CheckHostName($VpsHost) -eq 'Unknown') {
    throw 'Use a VPS IPv4 address or DNS hostname without a scheme, path, or port.'
}
$relayWorkspace = Split-Path $PSScriptRoot -Parent
$relayDirectory = Join-Path $relayWorkspace '.qunica/relay'
$clientPath = Join-Path $relayDirectory 'frpc.toml'
$serverPath = Join-Path $relayDirectory 'frps.toml'
if ((Test-Path -LiteralPath $clientPath) -or (Test-Path -LiteralPath $serverPath)) {
    throw 'Existing .qunica/relay configuration found; keep it or move it aside before generating new credentials.'
}
$bytes = New-Object byte[] 32
$random = [Security.Cryptography.RandomNumberGenerator]::Create()
try { $random.GetBytes($bytes) } finally { $random.Dispose() }
$relayToken = -join ($bytes | ForEach-Object { $_.ToString('x2') })
$client = @"
# Qunica Noise ciphertext only. Do not proxy the plaintext API port 8765.
serverAddr = "$VpsHost"
serverPort = $ControlPort
auth.method = "token"
auth.token = "$relayToken"
auth.additionalScopes = ["HeartBeats", "NewWorkConns"]
transport.tls.enable = false
loginFailExit = false

[[proxies]]
name = "qunica-mobile"
type = "tcp"
localIP = "127.0.0.1"
localPort = 8766
remotePort = $PhonePort
"@
$server = @"
bindAddr = "0.0.0.0"
bindPort = $ControlPort
proxyBindAddr = "0.0.0.0"
auth.method = "token"
auth.token = "$relayToken"
auth.additionalScopes = ["HeartBeats", "NewWorkConns"]
transport.tls.force = false
allowPorts = [{ single = $PhonePort }]
maxPortsPerClient = 1
"@
New-Item -ItemType Directory -Path $relayDirectory -Force | Out-Null
$utf8 = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText($clientPath, $client, $utf8)
[IO.File]::WriteAllText($serverPath, $server, $utf8)
Write-Output 'Created .qunica/relay/frpc.toml (PC) and frps.toml (VPS). Keep their shared token private.'
Write-Output "Desktop Phone connection > VPS relay > ${VpsHost}:$PhonePort"
