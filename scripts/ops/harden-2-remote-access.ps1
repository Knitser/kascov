# Step 2: close RDP and WinRM to the internet.
#
# RDP is the real exposure: 3389 accepts connections from anywhere, the admin
# username is public (it is in this repo's history), and the Security log shows
# thousands of failed logons with hundreds targeting that account by name.
# RDP has no key auth, so the only real fix is to stop strangers reaching it.
#
# Lockout safety: this scopes RDP to the IP you are CURRENTLY SSH'd in from,
# read live off the established connection rather than typed in — so it cannot
# be wrong. Your ISP hands out dynamic addresses (two different ranges seen in
# two weeks), so when it changes, RDP stops answering. That is expected: SSH is
# key-only and still open, so you SSH in and run harden-3-allow-rdp-from.ps1.
#
# Run AFTER step 1 is verified working:
#   powershell -ExecutionPolicy Bypass -File C:\Users\Administrator\harden-2-remote-access.ps1
# Undo:
#   Get-NetFirewallRule -DisplayName 'Remote Desktop*' | Set-NetFirewallRule -RemoteAddress Any

$ErrorActionPreference = "Stop"

# 1. discover the admin IP from the live SSH connection (never guessed)
$me = Get-NetTCPConnection -LocalPort 22 -State Established -ErrorAction SilentlyContinue |
      Select-Object -ExpandProperty RemoteAddress -Unique
if (-not $me) {
  "ABORT: no established SSH connection found, so I cannot determine your IP safely."
  "Run this over SSH, not from the console."
  exit 1
}
if ($me -is [array]) {
  "NOTE: more than one SSH source connected: {0}" -f ($me -join ', ')
  "All of them will be allowed for RDP."
}
"admin IP(s) detected from live SSH: {0}" -f ($me -join ', ')

# 2. verify key-only SSH is actually in effect before we rely on it as the fallback
$eff = & "C:\Windows\System32\OpenSSH\sshd.exe" -T 2>&1 |
       Where-Object { $_ -match '^passwordauthentication' }
if ($eff -notmatch 'no') {
  "ABORT: SSH still accepts passwords ($eff)."
  "Run harden-1-ssh-keyonly.ps1 first — otherwise locking RDP leaves a"
  "brute-forceable path as your only fallback."
  exit 1
}
"SSH is key-only, safe to proceed ($eff)"

# 3. scope RDP to the admin IP
Get-NetFirewallRule -Direction Inbound -Enabled True |
  Where-Object { $_.DisplayName -like 'Remote Desktop*' } |
  ForEach-Object {
    Set-NetFirewallRule -Name $_.Name -RemoteAddress $me
    "  scoped: {0}" -f $_.DisplayName
  }

# 4. WinRM 5985 is a remote-management port with no business being open to the
#    world; there is already a LocalSubnet rule, so drop the Any one to local.
Get-NetFirewallRule -Direction Inbound -Enabled True |
  Where-Object { $_.DisplayName -like 'Windows Remote Management*' } |
  ForEach-Object {
    $af = $_ | Get-NetFirewallAddressFilter
    if ($af.RemoteAddress -eq 'Any') {
      Set-NetFirewallRule -Name $_.Name -RemoteAddress LocalSubnet
      "  scoped to LocalSubnet: {0}" -f $_.DisplayName
    }
  }

# 5. show the resulting state for the ports that matter
""
"=== resulting inbound exposure ==="
Get-NetFirewallRule -Direction Inbound -Enabled True -Action Allow |
  Where-Object { $_.DisplayName -match 'kaspa|Remote Desktop|OpenSSH|Windows Remote Management' } |
  ForEach-Object {
    $pf = $_ | Get-NetFirewallPortFilter
    $af = $_ | Get-NetFirewallAddressFilter
    "{0,-36} port={1,-8} remote={2}" -f $_.DisplayName, ($pf.LocalPort -join ','), ($af.RemoteAddress -join ',')
  }
""
"Web ports 80/443 deliberately untouched: nothing proxies kascov.io, so every"
"visitor reaches this box directly. Restricting them would take the site down."
