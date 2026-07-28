# The un-lock-yourself-out helper.
#
# Your ISP rotates your address, so the RDP allowlist from step 2 goes stale.
# When RDP stops answering, SSH in (key-only, still open) and run this — it
# re-points the RDP rules at wherever you are now, read off the live SSH
# connection so it cannot be typed wrong.
#
#   ssh Administrator@157.90.7.39
#   powershell -ExecutionPolicy Bypass -File C:\Users\Administrator\harden-3-allow-rdp-from.ps1
#
# To allow an extra address as well (e.g. a phone hotspot you are about to
# switch to), pass it explicitly:
#   ... harden-3-allow-rdp-from.ps1 -Extra 203.0.113.7

param([string]$Extra)

$ErrorActionPreference = "Stop"

$me = Get-NetTCPConnection -LocalPort 22 -State Established -ErrorAction SilentlyContinue |
      Select-Object -ExpandProperty RemoteAddress -Unique
if (-not $me) { "ABORT: run this over SSH so I can see your current address."; exit 1 }

$allow = @($me)
if ($Extra) { $allow += $Extra }
"allowing RDP from: {0}" -f ($allow -join ', ')

Get-NetFirewallRule -Direction Inbound -Enabled True |
  Where-Object { $_.DisplayName -like 'Remote Desktop*' } |
  ForEach-Object {
    Set-NetFirewallRule -Name $_.Name -RemoteAddress $allow
    "  updated: {0}" -f $_.DisplayName
  }
"done - RDP now answers only to the address(es) above."
