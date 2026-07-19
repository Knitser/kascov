# kascov VPS provisioning — Windows Server 2022 (157.90.7.39, "the dedicated box")
# Phase 1: mainnet + testnet-10 archival Kaspa nodes as Windows services.
# Idempotent: safe to re-run; every step checks before acting.
#
# Layout: C:\kascov\bin (binaries, NSSM), N:\kaspa\{mainnet,testnet-10} (node data
# on the empty 1TB data drive). RPC ports are open to the internet during the
# bridge phase (the Cloud Run worker connects from Google's egress pool — same
# exposure every public Kaspa node has); Phase 3 moves the worker onto this box
# and RPC becomes localhost-only.

$ErrorActionPreference = 'Stop'
$bin = 'C:\kascov\bin'
$rkVersion = 'v2.0.1'
$rkZip = "https://github.com/kaspanet/rusty-kaspa/releases/download/$rkVersion/rusty-kaspa-$rkVersion-win64.zip"
$nssmZip = 'https://nssm.cc/release/nssm-2.24.zip'

New-Item -ItemType Directory -Force -Path $bin, 'N:\kaspa\mainnet', 'N:\kaspa\testnet-10' | Out-Null

# --- rusty-kaspa ---
if (-not (Test-Path "$bin\kaspad.exe")) {
    Write-Host "downloading rusty-kaspa $rkVersion..."
    Invoke-WebRequest -Uri $rkZip -OutFile "$env:TEMP\rk.zip" -UseBasicParsing
    Expand-Archive -Path "$env:TEMP\rk.zip" -DestinationPath "$env:TEMP\rk" -Force
    $kaspad = Get-ChildItem -Path "$env:TEMP\rk" -Recurse -Filter kaspad.exe | Select-Object -First 1
    Copy-Item -Path (Join-Path $kaspad.DirectoryName '*') -Destination $bin -Recurse -Force
    Write-Host "kaspad installed: $bin\kaspad.exe"
} else { Write-Host "kaspad already present" }

# --- NSSM (console apps as real services: restart-on-failure, SCM handshake) ---
if (-not (Test-Path "$bin\nssm.exe")) {
    Write-Host "downloading nssm..."
    Invoke-WebRequest -Uri $nssmZip -OutFile "$env:TEMP\nssm.zip" -UseBasicParsing
    Expand-Archive -Path "$env:TEMP\nssm.zip" -DestinationPath "$env:TEMP\nssm" -Force
    $nssm = Get-ChildItem -Path "$env:TEMP\nssm" -Recurse -Filter nssm.exe | Where-Object { $_.FullName -match 'win64' } | Select-Object -First 1
    Copy-Item $nssm.FullName "$bin\nssm.exe"
    Write-Host "nssm installed"
} else { Write-Host "nssm already present" }

# --- Windows Defender: exclude node data dirs (realtime scanning of RocksDB
#     write bursts cripples IBD throughput) ---
Add-MpPreference -ExclusionPath 'N:\kaspa' -ErrorAction SilentlyContinue

# --- services ---
function Ensure-KaspadService {
    param($name, $svcArgs)
    if (-not (Get-Service $name -ErrorAction SilentlyContinue)) {
        & "$bin\nssm.exe" install $name "$bin\kaspad.exe" $svcArgs
        & "$bin\nssm.exe" set $name AppStdout "N:\kaspa\$name.log"
        & "$bin\nssm.exe" set $name AppStderr "N:\kaspa\$name.log"
        & "$bin\nssm.exe" set $name AppRotateFiles 1
        & "$bin\nssm.exe" set $name AppRotateBytes 104857600
        & "$bin\nssm.exe" set $name AppExit Default Restart
        & "$bin\nssm.exe" set $name Start SERVICE_AUTO_START
        Write-Host "$name service created"
    } else { Write-Host "$name service exists" }
    Start-Service $name -ErrorAction SilentlyContinue
}

Ensure-KaspadService 'kaspad-mainnet' '--utxoindex --archival --appdir=N:\kaspa\mainnet --rpclisten-borsh=0.0.0.0:17110'
Ensure-KaspadService 'kaspad-tn10' '--testnet --netsuffix=10 --utxoindex --archival --appdir=N:\kaspa\testnet-10 --rpclisten-borsh=0.0.0.0:17210'

# --- firewall: P2P in (good citizenship) + RPC in (bridge phase; Phase 3 closes these) ---
foreach ($p in @(16111, 16211, 17110, 17210)) {
    $n = "kaspa-$p"
    if (-not (Get-NetFirewallRule -Name $n -ErrorAction SilentlyContinue)) {
        New-NetFirewallRule -Name $n -DisplayName $n -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort $p | Out-Null
        Write-Host "firewall: opened $p"
    }
}

Start-Sleep -Seconds 8
Get-Service kaspad-* | Format-Table Name, Status
Get-ChildItem N:\kaspa\*.log -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "--- $($_.Name) tail ---"
    Get-Content $_.FullName -Tail 3
}
