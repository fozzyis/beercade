<#
.SYNOPSIS
    Automated Hyper-V Generation 2 VHDX boot verification for x86_64 B.E.E.R.C.A.D.E. images.
.DESCRIPTION
    Provisions an isolated temporary Hyper-V Gen2 VM (Secure Boot Off, dynamic VHDX
    copied to %TEMP% and resized so DATA grows on first boot), boots it, waits for
    Hyper-V Heartbeat + KVP IPv4 discovery, probes payload readiness (B.O.S.S. HTTP
    :8080 or A.S.T.R.O. sshd :22 + optional SSH in-guest check), writes a structured
    JSON report, and cleans up the temporary VM and VHDX copy.

    Exit codes:
      0 - Verification passed (or -DryRun succeeded)
      1 - Boot, probe, or VHDX validation failure
      2 - Timed out waiting for guest readiness
      3 - Missing prerequisite (Hyper-V module, elevation, or .vhdx file)
#>
[CmdletBinding()]
param(
    [ValidateSet('astro', 'boss')]
    [string]$Payload = 'astro',

    [string]$VhdxPath = '',

    [int]$TimeoutSeconds = 0,

    [int]$MemoryMB = 0,

    [int]$CpuCount = 2,

    [int]$DiskGB = 16,

    [string]$SwitchName = '',

    [string]$SshKeyPath = '',

    [string]$ReportPath = '',

    [switch]$DryRun,

    [switch]$KeepVm
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if ([string]::IsNullOrWhiteSpace($VhdxPath)) {
    $VhdxPath = Join-Path $RepoRoot ("out\beercade-{0}-x86_64.vhdx" -f $Payload)
} elseif (-not [System.IO.Path]::IsPathRooted($VhdxPath)) {
    $VhdxPath = Join-Path $RepoRoot $VhdxPath
}

if ($TimeoutSeconds -le 0) {
    $TimeoutSeconds = if ($Payload -eq 'boss') { 480 } else { 300 }
}
if ($MemoryMB -le 0) {
    $MemoryMB = if ($Payload -eq 'boss') { 3072 } else { 2048 }
}
if ([string]::IsNullOrWhiteSpace($ReportPath)) {
    $ReportPath = Join-Path $RepoRoot ("out\hyperv-verify-x86_64-{0}.json" -f $Payload)
} elseif (-not [System.IO.Path]::IsPathRooted($ReportPath)) {
    $ReportPath = Join-Path $RepoRoot $ReportPath
}

function Write-Report {
    param(
        [hashtable]$Report,
        [string]$Path
    )
    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    $Report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Path -Encoding UTF8
}

function Test-VhdxMagic {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $false
    }
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $buf = New-Object byte[] 8
        $read = $stream.Read($buf, 0, 8)
        if ($read -lt 8) { return $false }
        $magic = [System.Text.Encoding]::ASCII.GetString($buf)
        return ($magic -eq 'vhdxfile')
    } finally {
        $stream.Dispose()
    }
}

function Test-IsElevated {
    $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object System.Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Test-TcpPort {
    param(
        [string]$HostName,
        [int]$Port,
        [int]$TimeoutMs = 2000
    )
    $client = New-Object System.Net.Sockets.TcpClient
    try {
        $iar = $client.BeginConnect($HostName, $Port, $null, $null)
        if (-not $iar.AsyncWaitHandle.WaitOne($TimeoutMs, $false)) {
            return $false
        }
        $client.EndConnect($iar)
        return $true
    } catch {
        return $false
    } finally {
        $client.Dispose()
    }
}

$requiredCmdlets = @('New-VM', 'Set-VMFirmware', 'Get-VMIntegrationService', 'Resize-VHD', 'Get-VMNetworkAdapter', 'Start-VM', 'Stop-VM', 'Remove-VM')
$missingCmdlets = @()
foreach ($cmd in $requiredCmdlets) {
    if (-not (Get-Command -Name $cmd -ErrorAction SilentlyContinue)) {
        $missingCmdlets += $cmd
    }
}

$vhdxExists = Test-Path -LiteralPath $VhdxPath -PathType Leaf
$vhdxMagicOk = if ($vhdxExists) { Test-VhdxMagic -Path $VhdxPath } else { $false }
$isElevated = Test-IsElevated

$report = [ordered]@{
    timestamp_utc   = (Get-Date).ToUniversalTime().ToString('o')
    payload         = $Payload
    mode            = if ($DryRun) { 'dry_run' } else { 'live' }
    vhdx_path       = $VhdxPath
    vhdx_exists     = $vhdxExists
    vhdx_magic_ok   = $vhdxMagicOk
    elevated        = $isElevated
    missing_cmdlets = $missingCmdlets
    generation      = 2
    secure_boot     = 'Off'
    memory_mb       = $MemoryMB
    cpu_count       = $CpuCount
    disk_gb         = $DiskGB
    status          = 'pending'
    heartbeat_ok    = $false
    guest_ip        = $null
    service_probe   = $null
    in_guest_check  = $null
    elapsed_seconds = 0
}

if ($missingCmdlets.Count -gt 0) {
    $report.status = 'prereq_missing'
    Write-Report -Report $report -Path $ReportPath
    Write-Error ("Hyper-V cmdlets missing: {0}" -f ($missingCmdlets -join ', '))
    exit 3
}

if ($DryRun) {
    if ($vhdxExists -and -not $vhdxMagicOk) {
        $report.status = 'invalid_vhdx'
        Write-Report -Report $report -Path $ReportPath
        Write-Error ("File exists at {0} but failed 'vhdxfile' magic header check." -f $VhdxPath)
        exit 1
    }
    $report.status = 'dry_run_ok'
    Write-Report -Report $report -Path $ReportPath
    Write-Output ("[PASS] Hyper-V Gen2 dry-run OK (payload={0}, cmdlets=ready, vhdx_exists={1}, magic_ok={2})." -f $Payload, $vhdxExists, $vhdxMagicOk)
    exit 0
}

if (-not $isElevated) {
    $report.status = 'prereq_missing'
    Write-Report -Report $report -Path $ReportPath
    Write-Error 'Live Hyper-V VM provisioning requires an elevated Administrator PowerShell session.'
    exit 3
}

if (-not $vhdxExists) {
    $report.status = 'prereq_missing'
    Write-Report -Report $report -Path $ReportPath
    Write-Error ("VHDX not found: {0} (build first via build_image.sh {1})" -f $VhdxPath, $Payload)
    exit 3
}

if (-not $vhdxMagicOk) {
    $report.status = 'invalid_vhdx'
    Write-Report -Report $report -Path $ReportPath
    Write-Error ("Invalid VHDX header (expected 'vhdxfile'): {0}" -f $VhdxPath)
    exit 1
}

$vmName = "beercade-verify-{0}-{1}-{2}" -f $Payload, $PID, ([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())
$tempVhdx = Join-Path $env:TEMP ("{0}.vhdx" -f $vmName)
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$exitCode = 1

try {
    Write-Output ("Copying {0} -> {1} ..." -f $VhdxPath, $tempVhdx)
    Copy-Item -LiteralPath $VhdxPath -Destination $tempVhdx -Force

    $vhdInfo = Get-VHD -Path $tempVhdx
    $targetBytes = [int64]$DiskGB * 1GB
    if ($vhdInfo.Size -lt $targetBytes) {
        Resize-VHD -Path $tempVhdx -SizeBytes $targetBytes
    }

    New-VM -Name $vmName -Generation 2 -MemoryStartupBytes ([int64]$MemoryMB * 1MB) -VHDPath $tempVhdx | Out-Null
    Set-VMProcessor -VMName $vmName -Count $CpuCount
    Set-VMMemory -VMName $vmName -DynamicMemoryEnabled $false
    Set-VMFirmware -VMName $vmName -EnableSecureBoot Off
    Set-VM -VMName $vmName -AutomaticCheckpointsEnabled $false

    if ([string]::IsNullOrWhiteSpace($SwitchName)) {
        $switches = @(Get-VMSwitch -ErrorAction SilentlyContinue)
        $preferred = $switches | Where-Object { $_.Name -eq 'Default Switch' } | Select-Object -First 1
        if (-not $preferred) {
            $preferred = $switches | Where-Object { $_.SwitchType -in @('External', 'Internal') } | Select-Object -First 1
        }
        if ($preferred) {
            $SwitchName = $preferred.Name
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($SwitchName)) {
        Connect-VMNetworkAdapter -VMName $vmName -SwitchName $SwitchName
    }

    Write-Output ("Starting Hyper-V Gen2 VM {0} (Secure Boot Off, switch={1}) ..." -f $vmName, $SwitchName)
    Start-VM -Name $vmName | Out-Null

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $guestIp = $null
    $serviceOk = $false

    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Seconds 3
        $vm = Get-VM -Name $vmName
        if ($vm.State -ne 'Running') {
            throw ("VM {0} transitioned unexpectedly to state '{1}'" -f $vmName, $vm.State)
        }

        $hb = Get-VMIntegrationService -VMName $vmName -Name 'Heartbeat' -ErrorAction SilentlyContinue
        if ($hb -and $hb.PrimaryStatusDescription -like 'OK*') {
            $report.heartbeat_ok = $true
        }

        if (-not $guestIp) {
            $ips = @((Get-VMNetworkAdapter -VMName $vmName).IPAddresses)
            $ipv4 = $ips | Where-Object {
                $_ -match '^\d{1,3}(\.\d{1,3}){3}$' -and $_ -notlike '169.254.*' -and $_ -ne '127.0.0.1'
            } | Select-Object -First 1
            if ($ipv4) {
                $guestIp = $ipv4
                $report.guest_ip = $guestIp
                Write-Output ("Guest IPv4 discovered via KVP: {0}" -f $guestIp)
            }
        }

        if ($report.heartbeat_ok -and $guestIp) {
            if ($Payload -eq 'boss') {
                try {
                    $health = Invoke-WebRequest -Uri ("http://{0}:8080/api/health" -f $guestIp) -UseBasicParsing -TimeoutSec 4
                    $ui = Invoke-WebRequest -Uri ("http://{0}:8080/" -f $guestIp) -UseBasicParsing -TimeoutSec 4
                    if ($health.StatusCode -eq 200 -and $health.Content.Trim() -eq 'ok' -and $ui.StatusCode -eq 200) {
                        $report.service_probe = 'boss_http_ok'
                        $serviceOk = $true
                        break
                    }
                } catch {
                    # Keep polling until timeout
                }
            } else {
                if (Test-TcpPort -HostName $guestIp -Port 22 -TimeoutMs 2000) {
                    $report.service_probe = 'astro_ssh_open'
                    $serviceOk = $true
                    break
                }
            }
        }
    }

    if (-not $serviceOk) {
        $report.status = 'timeout'
        $exitCode = 2
        throw ("Timed out after {0}s (heartbeat_ok={1}, guest_ip={2})" -f $TimeoutSeconds, $report.heartbeat_ok, $guestIp)
    }

    if (-not [string]::IsNullOrWhiteSpace($SshKeyPath) -and (Test-Path -LiteralPath $SshKeyPath -PathType Leaf) -and (Get-Command ssh -ErrorAction SilentlyContinue)) {
        $remoteCmd = if ($Payload -eq 'boss') {
            'systemctl is-active multi-user.target seatd.service boss-server.service boss-kiosk.service && lsmod | grep -q hv_vmbus && df -BG --output=size /data | awk "NR==2{exit (\$1+0>=5?0:1)}"'
        } else {
            'systemctl is-active multi-user.target astro-tune.service astro-daemon.service astro-frontend.service && pgrep -cx astro-shell >/dev/null && lsmod | grep -q hv_vmbus && df -BG --output=size /data | awk "NR==2{exit (\$1+0>=5?0:1)}"'
        }
        $sshOut = & ssh -i $SshKeyPath -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 ("root@{0}" -f $guestIp) $remoteCmd 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw ("In-guest SSH verification failed: {0}" -f ($sshOut -join "`n"))
        }
        $report.in_guest_check = 'passed'
    }

    $report.status = 'passed'
    $exitCode = 0
    Write-Output ("[PASS] Hyper-V Gen2 VHDX verification passed for {0} (ip={1}, probe={2})." -f $Payload, $guestIp, $report.service_probe)
} catch {
    if ($report.status -eq 'pending') {
        $report.status = 'failed'
    }
    $report['error'] = $_.Exception.Message
    Write-Error $_.Exception.Message
} finally {
    $stopwatch.Stop()
    $report.elapsed_seconds = [math]::Round($stopwatch.Elapsed.TotalSeconds, 2)
    Write-Report -Report $report -Path $ReportPath

    if (-not $KeepVm) {
        $existingVm = Get-VM -Name $vmName -ErrorAction SilentlyContinue
        if ($existingVm) {
            if ($existingVm.State -ne 'Off') {
                Stop-VM -Name $vmName -TurnOff -Force -ErrorAction SilentlyContinue
            }
            Remove-VM -Name $vmName -Force -ErrorAction SilentlyContinue
        }
        if (Test-Path -LiteralPath $tempVhdx -PathType Leaf) {
            Remove-Item -LiteralPath $tempVhdx -Force -ErrorAction SilentlyContinue
        }
    }
}

exit $exitCode
