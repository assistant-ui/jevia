[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$DefaultJeviaVersion = "0.1.8"
$JeviaVersion = if ($env:JEVIA_VERSION) { $env:JEVIA_VERSION } else { $DefaultJeviaVersion }
$JeviaRepository = "assistant-ui/jevia"
$LocalAppData = [Environment]::GetFolderPath("LocalApplicationData")
$InstallDirectory = if ($env:JEVIA_INSTALL_DIR) {
  $env:JEVIA_INSTALL_DIR
} else {
  Join-Path $LocalAppData "Jevia\bin"
}
$DownloadBaseUrl = if ($env:JEVIA_DOWNLOAD_BASE_URL) {
  $env:JEVIA_DOWNLOAD_BASE_URL.TrimEnd("/")
} else {
  "https://github.com/$JeviaRepository/releases/download/v$JeviaVersion"
}

function Stop-Install([string]$Message) {
  throw "Jevia installation failed: $Message"
}

function Get-JeviaTarget {
  if (-not [Environment]::Is64BitOperatingSystem) {
    Stop-Install "32-bit Windows is not supported"
  }

  $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
  if ($architecture -ne "X64") {
    Stop-Install "unsupported Windows architecture: $architecture"
  }

  return "x86_64-pc-windows-msvc"
}

function Confirm-JeviaChecksum([string]$AssetPath, [string]$ChecksumPath) {
  $checksumText = (Get-Content -LiteralPath $ChecksumPath -Raw).Trim()
  $expected = ($checksumText -split "\s+")[0].ToLowerInvariant()
  if ($expected -notmatch "^[0-9a-f]{64}$") {
    Stop-Install "release checksum is malformed"
  }

  $actual = (Get-FileHash -LiteralPath $AssetPath -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne $expected) {
    Stop-Install "checksum verification failed"
  }

  if ((Get-Item -LiteralPath $AssetPath).Length -eq 0) {
    Stop-Install "downloaded Jevia binary is empty"
  }
}

$TemporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) "jevia-install-$([Guid]::NewGuid().ToString('N'))"

try {
  $target = Get-JeviaTarget
  $assetName = "jevia-v$JeviaVersion-$target.exe"
  $downloadUrl = "$DownloadBaseUrl/$assetName"
  $assetPath = Join-Path $TemporaryDirectory $assetName
  $checksumPath = "$assetPath.sha256"

  New-Item -ItemType Directory -Path $TemporaryDirectory -Force | Out-Null
  Write-Host "Downloading Jevia $JeviaVersion for $target..."
  Invoke-WebRequest -Uri $downloadUrl -OutFile $assetPath -UseBasicParsing
  Invoke-WebRequest -Uri "$downloadUrl.sha256" -OutFile $checksumPath -UseBasicParsing
  Confirm-JeviaChecksum $assetPath $checksumPath

  New-Item -ItemType Directory -Path $InstallDirectory -Force | Out-Null
  $temporaryDestination = Join-Path $InstallDirectory ".jevia.install.$PID.exe"
  $installedBinary = Join-Path $InstallDirectory "jevia.exe"
  Copy-Item -LiteralPath $assetPath -Destination $temporaryDestination -Force
  Move-Item -LiteralPath $temporaryDestination -Destination $installedBinary -Force

  Write-Host "Installed Jevia $JeviaVersion to $installedBinary"
  $pathEntries = ($env:PATH -split [IO.Path]::PathSeparator).TrimEnd("\")
  if ($pathEntries -notcontains $InstallDirectory.TrimEnd("\")) {
    Write-Host "Add $InstallDirectory to PATH, open a new terminal, then run ``jevia init``."
  } else {
    Write-Host "Run ``jevia init`` to get started."
  }
} finally {
  if (Test-Path -LiteralPath $TemporaryDirectory) {
    Remove-Item -LiteralPath $TemporaryDirectory -Recurse -Force
  }
}
