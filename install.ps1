$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Write-Info { param([string]$Message) Write-Host "$Message" -ForegroundColor Cyan }
function Write-Success { param([string]$Message) Write-Host "$Message" -ForegroundColor Green }
function Write-Warn { param([string]$Message) Write-Host "WARNING: $Message" -ForegroundColor Yellow }
function Write-Err { param([string]$Message) Write-Host "ERROR: $Message" -ForegroundColor Red; exit 1 }

$InstallDir = "$env:LOCALAPPDATA\hdhub4u-tui"
$ExePath = "$InstallDir\hdhub4u-tui.exe"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("hdhub4u-tui-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

Write-Info "Fetching latest version information..."
try {
    $Request = [System.Net.WebRequest]::Create("https://github.com/ShubhamPP04/hdhub4u-tui/releases/latest")
    $Request.AllowAutoRedirect = $false
    $Response = $Request.GetResponse()
    $Location = $Response.Headers["Location"]
    $Version = $Location.Split('/')[-1]
    $Response.Close()
    if (-not $Version) { throw "Version not found from redirect." }
} catch {
    Write-Err "Failed to fetch latest version from GitHub API. Please check your internet connection."
}

$IsUpdate = $false
$CurrentVersion = "unknown"
if (Test-Path $ExePath) {
    try {
        $CurrentVersionOutput = (& $ExePath --version 2>&1 | Out-String)
        if ($CurrentVersionOutput -match "hdhub4u-tui\s+([\d\.]+)") {
            $CurrentVersion = $matches[1]
            if ("v$CurrentVersion" -eq $Version) {
                Write-Success "You already have the latest version ($Version) installed."
                exit 0
            }
        }
        Write-Info "Updating HDHub4u-TUI from v$CurrentVersion to $Version..."
        $IsUpdate = $true
        
        $RunningProcesses = Get-Process -Name "hdhub4u-tui" -ErrorAction SilentlyContinue
        if ($RunningProcesses) {
            Write-Info "Stopping running instances of hdhub4u-tui..."
            $RunningProcesses | Stop-Process -Force
            Start-Sleep -Seconds 1
        }
    } catch {
        Write-Info "Updating HDHub4u-TUI to $Version..."
        $IsUpdate = $true
    }
} else {
    Write-Info "Installing HDHub4u-TUI $Version..."
}

$Architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($Architecture -eq "ARM64") {
    $ArchiveName = "hdhub4u_Windows_arm64.zip"
} elseif ($Architecture -eq "AMD64") {
    $ArchiveName = "hdhub4u_Windows_x64.zip"
} else {
    Write-Err "Unsupported Windows architecture: $Architecture"
}
$ZipFile = Join-Path $TempDir $ArchiveName
$ChecksumFile = Join-Path $TempDir "SHA256SUMS"
$BaseUrl = "https://github.com/ShubhamPP04/hdhub4u-tui/releases/download/$Version"
$Url = "$BaseUrl/$ArchiveName"

Write-Info "Downloading release archive..."
try {
    Invoke-WebRequest -Uri $Url -OutFile $ZipFile -UseBasicParsing
    Invoke-WebRequest -Uri "$BaseUrl/SHA256SUMS" -OutFile $ChecksumFile -UseBasicParsing
} catch {
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Err "Download failed. Please check your internet connection."
}

$ChecksumLine = Get-Content $ChecksumFile | Where-Object { $_ -match "\s+$([regex]::Escape($ArchiveName))$" } | Select-Object -First 1
if (-not $ChecksumLine) {
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Err "Release checksum is missing for $ArchiveName."
}
$ExpectedHash = ($ChecksumLine -split '\s+')[0]
$ActualHash = (Get-FileHash -Path $ZipFile -Algorithm SHA256).Hash
if ($ActualHash -ne $ExpectedHash) {
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Err "Checksum verification failed."
}

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

Write-Info "Extracting files..."
try {
    Expand-Archive -Path $ZipFile -DestinationPath $InstallDir -Force
} catch {
    Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Err "Failed to extract archive."
}

Remove-Item $TempDir -Recurse -Force

$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notmatch [regex]::Escape($InstallDir)) {
    Write-Info "Adding $InstallDir to PATH..."
    $NewPath = "$UserPath;$InstallDir"
    [Environment]::SetEnvironmentVariable("PATH", $NewPath, "User")
    Write-Warn "Please restart your PowerShell window for the PATH changes to take effect."
}

if ($IsUpdate) {
    Write-Success "Update complete! Run 'hdhub4u-tui' to start."
} else {
    Write-Success "Installation complete! Run 'hdhub4u-tui' to start."
}
