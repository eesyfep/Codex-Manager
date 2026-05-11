[CmdletBinding()]
param(
    [int]$FrontendPort = 3005,
    [int]$ServicePort = 48762,
    [switch]$UseProductionGatewayPort
)

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$appsDir = Join-Path $repoRoot "apps"
$tauriDir = Join-Path $appsDir "src-tauri"
$qaDir = Join-Path $repoRoot ".qa"
$dbPath = Join-Path $qaDir "codexmanager-router-v1.db"
$rpcTokenPath = Join-Path $qaDir "codexmanager-router-v1.rpc-token"
$workspaceTargetDir = Join-Path $qaDir "codexrouter-target"
$targetDir = Join-Path $qaDir "codexrouter-tauri-target"
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$pnpm = (Get-Command "pnpm.ps1" -ErrorAction SilentlyContinue).Source
if ([string]::IsNullOrWhiteSpace($pnpm)) {
    $pnpm = (Get-Command "pnpm" -ErrorAction SilentlyContinue).Source
}
$msysPath = "C:\msys64\mingw64\bin"

if ($UseProductionGatewayPort) {
    $ServicePort = 48760
}

New-Item -ItemType Directory -Force -Path $qaDir | Out-Null

function Test-PortListening {
    param([int]$Port)
    $connections = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue
    return $null -ne $connections
}

function Get-PortOwnerText {
    param([int]$Port)
    $connections = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue
    if (-not $connections) {
        return ""
    }
    $items = foreach ($connection in $connections) {
        $process = Get-Process -Id $connection.OwningProcess -ErrorAction SilentlyContinue
        $processName = "unknown"
        if ($null -ne $process -and -not [string]::IsNullOrWhiteSpace($process.ProcessName)) {
            $processName = $process.ProcessName
        }
        "{0}/{1}" -f $processName, $connection.OwningProcess
    }
    return ($items | Sort-Object -Unique) -join ", "
}

function Test-DevDesktopRunning {
    $expectedPrefix = (Resolve-Path $targetDir -ErrorAction SilentlyContinue)
    if ($null -eq $expectedPrefix) {
        return $false
    }
    $expectedRoot = $expectedPrefix.Path
    $processes = Get-Process -Name "CodexManager" -ErrorAction SilentlyContinue
    foreach ($process in $processes) {
        if ($process.Path -and $process.Path.StartsWith($expectedRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    return $false
}

if ((Test-PortListening -Port $ServicePort) -and -not $UseProductionGatewayPort) {
    if (Test-DevDesktopRunning) {
        Write-Host "Codex Router Desktop Dev is already running on 127.0.0.1:$ServicePort."
        exit 0
    }
    throw "Dev service port 127.0.0.1:$ServicePort is already in use: $(Get-PortOwnerText -Port $ServicePort). Close the owner or pass -ServicePort."
}

if ($UseProductionGatewayPort -and (Test-PortListening -Port 48760)) {
    throw "Production gateway port 127.0.0.1:48760 is already in use: $(Get-PortOwnerText -Port 48760). Close the running CodexManager before takeover validation."
}

if (-not (Test-PortListening -Port $FrontendPort)) {
    Write-Host "Starting frontend dev server: http://127.0.0.1:$FrontendPort"
    if ([string]::IsNullOrWhiteSpace($pnpm)) {
        throw "pnpm was not found on PATH."
    }
    $frontendCommand = "& '$pnpm' -C '$appsDir' run dev:desktop"
    Start-Process -FilePath "powershell.exe" `
        -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $frontendCommand) `
        -WorkingDirectory $repoRoot `
        -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $qaDir "frontend-dev.out.log") `
        -RedirectStandardError (Join-Path $qaDir "frontend-dev.err.log") | Out-Null

    $deadline = (Get-Date).AddSeconds(45)
    while ((Get-Date) -lt $deadline) {
        if (Test-PortListening -Port $FrontendPort) {
            break
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not (Test-PortListening -Port $FrontendPort)) {
        throw "Frontend dev server did not listen on 127.0.0.1:$FrontendPort within 45 seconds."
    }
} else {
    Write-Host "Reusing frontend dev server: http://127.0.0.1:$FrontendPort ($(Get-PortOwnerText -Port $FrontendPort))"
}

$tauriConfigPath = Join-Path $tauriDir "tauri.router-dev.conf.json"
$env:TAURI_CONFIG = Get-Content -Raw -Encoding utf8 $tauriConfigPath
$env:CODEXMANAGER_SERVICE_ADDR = "127.0.0.1:$ServicePort"
$env:CODEXMANAGER_DB_PATH = $dbPath
$env:CODEXMANAGER_RPC_TOKEN_FILE = $rpcTokenPath
$env:CODEXMANAGER_IMPORT_SOURCE_PATH = "C:\Users\WIN\AppData\Roaming\com.codexmanager.desktop\codexmanager.db"
$env:CODEXMANAGER_CODEX_STATE_DB_PATH = Join-Path $env:USERPROFILE ".codex\state_5.sqlite"
$env:CARGO_TARGET_DIR_WORKSPACE = $workspaceTargetDir
$env:CARGO_TARGET_DIR = $targetDir
if (Test-Path $msysPath) {
    $env:PATH = "$msysPath;$env:PATH"
}

Write-Host "Starting desktop shell: Codex Router Desktop Dev"
Write-Host "Service address: 127.0.0.1:$ServicePort"
Write-Host "Database: $dbPath"
Write-Host "Codex state: $env:CODEXMANAGER_CODEX_STATE_DB_PATH"

& $cargo +stable-x86_64-pc-windows-gnu run --manifest-path (Join-Path $tauriDir "Cargo.toml")

