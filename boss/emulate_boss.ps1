param(
    [string]$Platform = "rpi5",
    [switch]$Headless,
    [string]$Image = "",
    [string]$Kernel = "",
    [string]$Dtb = ""
)

Write-Host "=================================================================" -ForegroundColor Cyan
Write-Host "   🚀 Starting B.O.S.S. ARM64 SBC Emulator ($Platform)           " -ForegroundColor Green
Write-Host "=================================================================" -ForegroundColor Cyan

if (-not $Headless) {
    Write-Host "   • Web Kiosk Screen : http://localhost:6080/vnc.html" -ForegroundColor Yellow
    Write-Host "   • Web Kiosk UI     : http://localhost:8989" -ForegroundColor Yellow
    Write-Host "=================================================================" -ForegroundColor Cyan

    Start-Job -ScriptBlock {
        Start-Sleep -Seconds 4
        Start-Process "http://localhost:6080/vnc.html?autoconnect=true&resize=remote"
    } | Out-Null
}

$argsList = @("/mnt/d/data/dev/beercade/BOSS/emulate_boss.sh", "--platform", $Platform)
if ($Headless) { $argsList += "--headless" }
if ($Image -ne "") { $argsList += @("--image", $Image) }
if ($Kernel -ne "") { $argsList += @("--kernel", $Kernel) }
if ($Dtb -ne "") { $argsList += @("--dtb", $Dtb) }

wsl -u root -d kali-linux bash @argsList
