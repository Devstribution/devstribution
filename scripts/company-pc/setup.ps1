<#
.SYNOPSIS
    Turns a throwaway Windows VM into a stand-in for a locked-down company PC (roadmap N4).

.DESCRIPTION
    Run as administrator, on a VM you can throw away: it changes machine-wide settings. Each part
    is a switch, so you can set up only what one check needs; -All sets up everything, -Undo
    takes everything back out.

      -User       a standard (non-admin) local account, `dsuser`, to run devstribution as.
      -AppLocker  AppLocker's default rules, enforced: programs, scripts and installers only
                  from Program Files and Windows (administrators: anywhere). -Dll adds DLL rules,
                  which is what WDAC does to the `.pyd` files of dependencies. Needs Windows
                  Enterprise or Education (the free evaluation VMs are Enterprise).
      -Proxy      mitmproxy as a TLS-inspecting proxy on 127.0.0.1:8080, its certificate trusted
                  by the machine (as a company's would be), and HTTPS_PROXY / HTTP_PROXY set for
                  every user, which uv follows.
      -Share      a trust folder shared as \\<this PC>\ds-trust (only administrators can change
                  it) and one shared as \\<this PC>\ds-trust-open (users can change it).
      -Policy     managed mode: HKLM\SOFTWARE\Policies\devstribution, with the trust folder on
                  the share (or -TrustFolder <path>) and a contact.

    Then follow docs/company-pc.md.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\company-pc\setup.ps1 -All
    powershell -ExecutionPolicy Bypass -File scripts\company-pc\setup.ps1 -Undo
#>
[CmdletBinding()]
param(
    [switch]$All,
    [switch]$User,
    [switch]$AppLocker,
    [switch]$Dll,
    [switch]$AuditOnly,
    [switch]$Proxy,
    [switch]$Share,
    [switch]$Policy,
    [string]$TrustFolder,
    [switch]$Undo
)

$ErrorActionPreference = 'Stop'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this as administrator, on a VM you can throw away.'
}
if ($All) { $User = $AppLocker = $Proxy = $Share = $Policy = $true }

$AccountName = 'dsuser'
$ShareRoot = 'C:\ds-share'
$PolicyKey = 'HKLM:\SOFTWARE\Policies\devstribution'
$ProxyTask = 'devstribution mitmproxy'
$ProxyUrl = 'http://127.0.0.1:8080'
$MitmDir = 'C:\ProgramData\ds-mitmproxy'

function Step($text) { Write-Host "== $text" -ForegroundColor Cyan }

# ---------------------------------------------------------------- undo
if ($Undo) {
    Step 'Managed mode off'
    Remove-Item $PolicyKey -Recurse -Force -ErrorAction SilentlyContinue
    Step 'Shares removed'
    foreach ($name in 'ds-trust', 'ds-trust-open') {
        Remove-SmbShare -Name $name -Force -ErrorAction SilentlyContinue
    }
    Remove-Item $ShareRoot -Recurse -Force -ErrorAction SilentlyContinue
    Step 'Proxy removed'
    Unregister-ScheduledTask -TaskName $ProxyTask -Confirm:$false -ErrorAction SilentlyContinue
    Get-Process mitmdump -ErrorAction SilentlyContinue | Stop-Process -Force
    foreach ($name in 'HTTPS_PROXY', 'HTTP_PROXY') {
        [Environment]::SetEnvironmentVariable($name, $null, 'Machine')
    }
    Get-ChildItem Cert:\LocalMachine\Root | Where-Object Subject -Like '*mitmproxy*' | Remove-Item
    Step 'AppLocker rules cleared'
    $empty = '<AppLockerPolicy Version="1" />'
    $file = Join-Path $env:TEMP 'ds-applocker-empty.xml'
    Set-Content -Path $file -Value $empty -Encoding UTF8
    Set-AppLockerPolicy -XmlPolicy $file
    Step "The account $AccountName is kept: remove it with  Remove-LocalUser $AccountName"
    return
}

# ---------------------------------------------------------------- a standard user
if ($User) {
    Step "Standard user $AccountName"
    if (Get-LocalUser -Name $AccountName -ErrorAction SilentlyContinue) {
        Write-Host "   already there"
    } else {
        $password = Read-Host "Password for $AccountName" -AsSecureString
        New-LocalUser -Name $AccountName -Password $password -FullName 'devstribution test user' `
            -Description 'Standard user for devstribution checks' -PasswordNeverExpires | Out-Null
        Add-LocalGroupMember -Group (Get-LocalGroup -SID 'S-1-5-32-545') -Member $AccountName
    }
}

# ---------------------------------------------------------------- AppLocker
if ($AppLocker) {
    Step 'AppLocker default rules'
    $edition = (Get-CimInstance Win32_OperatingSystem).Caption
    if ($edition -notmatch 'Enterprise|Education|Server') {
        Write-Warning "$edition does not enforce AppLocker: use a Windows Enterprise VM."
    }
    $mode = if ($AuditOnly) { 'AuditOnly' } else { 'Enabled' }
    # Everyone: Program Files and Windows. Administrators (S-1-5-32-544): everything.
    function Rules($type, $ids, $paths) {
        $rules = for ($i = 0; $i -lt $paths.Count; $i++) {
            $sid = if ($paths[$i] -eq '*') { 'S-1-5-32-544' } else { 'S-1-1-0' }
            "<FilePathRule Id=""$($ids[$i])"" Name=""(Default Rule) $($paths[$i])"" Description="""" UserOrGroupSid=""$sid"" Action=""Allow""><Conditions><FilePathCondition Path=""$($paths[$i])"" /></Conditions></FilePathRule>"
        }
        "<RuleCollection Type=""$type"" EnforcementMode=""$mode"">$($rules -join '')</RuleCollection>"
    }
    $collections = @(
        (Rules 'Exe' @('921cc481-6e17-4653-8f75-050b80acca20', 'a61c8b2c-a319-4cd0-9690-d2177cad7b51', 'fd686d83-a829-4351-8ff4-27c7de5755d2') @('%PROGRAMFILES%\*', '%WINDIR%\*', '*')),
        (Rules 'Script' @('06dce67b-934c-454f-a263-2515c8796a5d', '9428c672-5fc3-47f4-808a-a0011f36dd2c', 'ed97d0cb-15ff-430f-b82c-8d7832957725') @('%PROGRAMFILES%\*', '%WINDIR%\*', '*')),
        (Rules 'Msi' @('5b290184-345a-4453-b184-45305f6d9a54', '64ad46ff-0d71-4fa0-a30b-3f3d30c5433d', 'b7af7102-efde-4369-8a89-7a6a392d1473') @('%WINDIR%\Installer\*', '%PROGRAMFILES%\*', '*'))
    )
    if ($Dll) {
        $collections += Rules 'Dll' @('3737732c-99b7-41d4-9037-9cddfb0de0d0', 'bac4b0bf-6f1b-40e8-8627-8545fa89c8b6', 'fe64f59f-6fca-45e5-a731-0f6715327c38') @('%PROGRAMFILES%\*', '%WINDIR%\*', '*')
    }
    $xml = "<AppLockerPolicy Version=""1"">$($collections -join '')</AppLockerPolicy>"
    $file = Join-Path $env:TEMP 'ds-applocker.xml'
    Set-Content -Path $file -Value $xml -Encoding UTF8
    Set-AppLockerPolicy -XmlPolicy $file
    # AppLocker enforces only while the Application Identity service runs.
    sc.exe config AppIDSvc start= auto | Out-Null
    Start-Service AppIDSvc
    Write-Host "   rules $mode; sign out and in again for them to apply"
}

# ---------------------------------------------------------------- TLS-inspecting proxy
if ($Proxy) {
    Step 'mitmproxy as a TLS-inspecting proxy'
    $mitmdump = (Get-Command mitmdump -ErrorAction SilentlyContinue).Source
    if (-not $mitmdump) {
        $candidate = Join-Path $env:ProgramFiles 'mitmproxy\bin\mitmdump.exe'
        if (-not (Test-Path $candidate)) {
            Write-Host '   installing mitmproxy with winget'
            winget install --id mitmproxy.mitmproxy --exact --silent --accept-package-agreements --accept-source-agreements --scope machine
        }
        $mitmdump = $candidate
    }
    if (-not (Test-Path $mitmdump)) { throw "mitmdump not found at ${mitmdump}: install mitmproxy for all users first." }
    New-Item -ItemType Directory $MitmDir -Force | Out-Null
    # The proxy's own folder: its private key stays with administrators.
    icacls $MitmDir /inheritance:r /grant:r 'Administrators:(OI)(CI)F' 'SYSTEM:(OI)(CI)F' | Out-Null
    $cert = Join-Path $MitmDir 'mitmproxy-ca-cert.cer'
    if (-not (Test-Path $cert)) {
        # The first start writes the certificate authority.
        $p = Start-Process $mitmdump -ArgumentList "--set confdir=$MitmDir --listen-port 8080" -PassThru -WindowStyle Hidden
        for ($i = 0; $i -lt 30 -and -not (Test-Path $cert); $i++) { Start-Sleep -Milliseconds 500 }
        Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    }
    if (-not (Test-Path $cert)) { throw 'mitmproxy did not write its certificate.' }
    Import-Certificate -FilePath $cert -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
    $action = New-ScheduledTaskAction -Execute $mitmdump -Argument "--set confdir=$MitmDir --listen-host 127.0.0.1 --listen-port 8080"
    $trigger = New-ScheduledTaskTrigger -AtStartup
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -AllowStartIfOnBatteries
    Register-ScheduledTask -TaskName $ProxyTask -Action $action -Trigger $trigger -Settings $settings `
        -User 'SYSTEM' -RunLevel Highest -Force | Out-Null
    Start-ScheduledTask -TaskName $ProxyTask
    foreach ($name in 'HTTPS_PROXY', 'HTTP_PROXY') {
        [Environment]::SetEnvironmentVariable($name, $ProxyUrl, 'Machine')
    }
    Write-Host "   proxy on $ProxyUrl, its certificate trusted; sign out and in again for HTTPS_PROXY"
    Write-Host "   to see what goes through it:  mitmweb --set confdir=$MitmDir (stop the task first)"
}

# ---------------------------------------------------------------- trust folders on a share
if ($Share) {
    Step "Trust folders shared from $ShareRoot"
    $locked = Join-Path $ShareRoot 'trust'
    $open = Join-Path $ShareRoot 'trust-open'
    New-Item -ItemType Directory $locked, $open -Force | Out-Null
    # Only administrators change `trust`; users can change `trust-open`, which must approve no one.
    icacls $ShareRoot /inheritance:r /grant:r 'Administrators:(OI)(CI)F' 'SYSTEM:(OI)(CI)F' 'Users:(OI)(CI)RX' | Out-Null
    icacls $open /grant 'Users:(OI)(CI)M' | Out-Null
    foreach ($s in @(@('ds-trust', $locked), @('ds-trust-open', $open))) {
        if (-not (Get-SmbShare -Name $s[0] -ErrorAction SilentlyContinue)) {
            New-SmbShare -Name $s[0] -Path $s[1] -FullAccess 'Administrators' -ChangeAccess 'Users' | Out-Null
        }
    }
    Write-Host "   \\$env:COMPUTERNAME\ds-trust (administrators only), \\$env:COMPUTERNAME\ds-trust-open (users can change)"
}

# ---------------------------------------------------------------- managed mode
if ($Policy) {
    Step 'Managed mode'
    $folder = if ($TrustFolder) { $TrustFolder } else { "\\$env:COMPUTERNAME\ds-trust" }
    New-Item -Path $PolicyKey -Force | Out-Null
    Set-ItemProperty $PolicyKey -Name TrustFolder -Value $folder
    Set-ItemProperty $PolicyKey -Name Contact -Value 'the test service desk (it-help@example.com)'
    Write-Host "   TrustFolder = $folder"
}

Step 'Done. Sign in as the test user and follow docs/company-pc.md.'
