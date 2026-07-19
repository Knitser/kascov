# kascov VPS build toolchain — one-time setup so the box builds its own binaries.
# Installs: VS Build Tools (MSVC linker), rustup (msvc toolchain), git, protoc.
# Then clones the repo and builds the worker. Idempotent; ~30-60 min first run.

$ErrorActionPreference = 'Stop'
$tools = 'C:\kascov\tools'
New-Item -ItemType Directory -Force -Path $tools, 'C:\kascov\src' | Out-Null
$env:PATH = "$env:USERPROFILE\.cargo\bin;$tools\protoc\bin;C:\Program Files\Git\cmd;$env:PATH"

# --- VS Build Tools (MSVC) ---
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$haveMsvc = (Test-Path $vswhere) -and (& $vswhere -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -latest -property installationPath)
if (-not $haveMsvc) {
    Write-Host 'installing VS Build Tools (this is the long one, ~15-30 min)...'
    Invoke-WebRequest -Uri 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile "$env:TEMP\vs_bt.exe" -UseBasicParsing
    Start-Process -Wait -FilePath "$env:TEMP\vs_bt.exe" -ArgumentList `
        '--quiet', '--wait', '--norestart', '--nocache', `
        '--add', 'Microsoft.VisualStudio.Workload.VCTools', `
        '--add', 'Microsoft.VisualStudio.Component.Windows11SDK.22621', `
        '--includeRecommended'
    Write-Host 'VS Build Tools installed'
} else { Write-Host 'MSVC already present' }

# --- rustup / cargo ---
if (-not (Test-Path "$env:USERPROFILE\.cargo\bin\cargo.exe")) {
    Write-Host 'installing rustup...'
    Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile "$env:TEMP\rustup-init.exe" -UseBasicParsing
    & "$env:TEMP\rustup-init.exe" -y --default-toolchain stable-x86_64-pc-windows-msvc --profile minimal
    Write-Host 'rustup installed'
} else { Write-Host 'cargo already present' }

# --- git ---
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Host 'installing git...'
    Invoke-WebRequest -Uri 'https://github.com/git-for-windows/git/releases/download/v2.46.0.windows.1/Git-2.46.0-64-bit.exe' -OutFile "$env:TEMP\git.exe" -UseBasicParsing
    Start-Process -Wait -FilePath "$env:TEMP\git.exe" -ArgumentList '/VERYSILENT', '/NORESTART', '/NOCANCEL'
    Write-Host 'git installed'
} else { Write-Host 'git already present' }

# --- protoc (kaspa grpc build deps) ---
if (-not (Test-Path "$tools\protoc\bin\protoc.exe")) {
    Write-Host 'installing protoc...'
    Invoke-WebRequest -Uri 'https://github.com/protocolbuffers/protobuf/releases/download/v27.2/protoc-27.2-win64.zip' -OutFile "$env:TEMP\protoc.zip" -UseBasicParsing
    Expand-Archive -Path "$env:TEMP\protoc.zip" -DestinationPath "$tools\protoc" -Force
    Write-Host 'protoc installed'
} else { Write-Host 'protoc already present' }

# --- clone + build ---
$env:PATH = "$env:USERPROFILE\.cargo\bin;$tools\protoc\bin;C:\Program Files\Git\cmd;$env:PATH"
if (-not (Test-Path 'C:\kascov\src\kascov\.git')) {
    git clone --depth 50 https://github.com/Knitser/kascov.git C:\kascov\src\kascov
} else {
    Set-Location C:\kascov\src\kascov; git pull
}
Set-Location C:\kascov\src\kascov
Write-Host 'building the worker (first build ~10-20 min)...'
cargo build --release -p kascov 2>&1 | Select-Object -Last 5
Copy-Item target\release\kascov.exe C:\kascov\bin\kascov.exe -Force
Write-Host ("worker built: " + (Get-Item C:\kascov\bin\kascov.exe).Length + " bytes")
