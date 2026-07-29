# Step 1 of the remote-access hardening: make SSH key-only.
#
# Why first: SSH becomes the permanent safety net. A key cannot be brute-forced
# in practice, so SSH can stay open to the internet while RDP gets locked down
# in step 2 - and if a future IP change ever locks you out of RDP, SSH is how
# you get back in to fix it.
#
# Safe by construction: refuses to run without a working key installed,
# validates the config with `sshd -T` BEFORE restarting, and restores the
# backup automatically if the parse fails. RDP stays open while you run this,
# so you always have a second way in.
#
# Run:  powershell -ExecutionPolicy Bypass -File <admin-home>\harden-1-ssh-keyonly.ps1
# Undo: copy C:\ProgramData\ssh\sshd_config.bak-keyonly over sshd_config, then
#       Restart-Service sshd

$ErrorActionPreference = "Stop"
$cfg    = "C:\ProgramData\ssh\sshd_config"
$backup = "C:\ProgramData\ssh\sshd_config.bak-keyonly"
$sshd   = "C:\Windows\System32\OpenSSH\sshd.exe"

$ak = "C:\ProgramData\ssh\administrators_authorized_keys"
if (-not (Test-Path $ak) -or -not (Get-Content $ak | Where-Object { $_ -match '^(ssh|ecdsa)-' })) {
  "ABORT: no usable key in administrators_authorized_keys."
  "Disabling password auth now would lock you out of SSH. Install your public key first."
  exit 1
}
"key check OK: {0} key line(s) installed" -f ((Get-Content $ak | Where-Object { $_ -match '^(ssh|ecdsa)-' }).Count)

if (-not (Test-Path $backup)) { Copy-Item $cfg $backup }
"backup: $backup"

$lines = Get-Content $cfg
function Set-Directive($lines, $name, $value) {
  $re = "^\s*#?\s*$name\s+"
  if ($lines -match $re) { return $lines -replace ($re + ".*"), "$name $value" }
  return $lines + "$name $value"
}
$lines = Set-Directive $lines "PasswordAuthentication" "no"
$lines = Set-Directive $lines "PubkeyAuthentication"   "yes"
$lines = Set-Directive $lines "PermitEmptyPasswords"   "no"
Set-Content -Path $cfg -Value $lines -Encoding UTF8

$test = & $sshd -T 2>&1
if ($LASTEXITCODE -ne 0) {
  Copy-Item $backup $cfg -Force
  "ABORT: sshd rejected the config. Backup restored, nothing changed. Error:"
  $test | Select-Object -First 10
  exit 1
}
"config validates OK"
$test | Where-Object { $_ -match '^(passwordauthentication|pubkeyauthentication|permitemptypasswords)' } |
  ForEach-Object { "  effective: $_" }

Restart-Service sshd
Start-Sleep -Seconds 3
"sshd status: {0}" -f (Get-Service sshd).Status
""
# Compose the verify hint from the live connection rather than hardcoding the
# box's address and admin name into a public repo.
$here = Get-NetTCPConnection -LocalPort 22 -State Established -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty LocalAddress -Unique | Select-Object -First 1
"NEXT: from your workstation, open a NEW terminal and run"
if ($here) { "  ssh {0}@{1} whoami" -f $env:USERNAME, $here }
else       { "  ssh <admin>@<this-box> whoami" }
"Do not close your current session until that succeeds."
