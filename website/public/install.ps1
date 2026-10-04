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

function Confirm-JeviaDestination([string]$Path) {
  # Inspect the entry itself so directories, junctions, and dangling links are
  # never mistaken for a replaceable executable.
  $entry = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
  if ($null -ne $entry -and (
    $entry.PSIsContainer -or
    ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
    $entry -isnot [IO.FileInfo]
  )) {
    Stop-Install "install destination is not a regular file; choose another install directory"
  }
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
$temporaryDestination = $null
$installedBinary = Join-Path $InstallDirectory "jevia.exe"

try {
  $target = Get-JeviaTarget
  Confirm-JeviaDestination $installedBinary
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
  $temporaryDestination = Join-Path $InstallDirectory ".jevia.install.$([Guid]::NewGuid().ToString('N')).exe"
  [IO.File]::Copy($assetPath, $temporaryDestination, $false)
  Confirm-JeviaDestination $installedBinary
  # File APIs reject a directory destination instead of copying inside it.
  if ([IO.File]::Exists($installedBinary)) {
    [IO.File]::Replace($temporaryDestination, $installedBinary, $null)
  } else {
    [IO.File]::Move($temporaryDestination, $installedBinary)
  }
  $temporaryDestination = $null
  Confirm-JeviaDestination $installedBinary
  if (-not [IO.File]::Exists($installedBinary)) { Stop-Install "installed binary could not be confirmed" }

  Write-Host "Installed Jevia $JeviaVersion to $installedBinary"
  $pathEntries = ($env:PATH -split [IO.Path]::PathSeparator).TrimEnd("\")
  if ($pathEntries -notcontains $InstallDirectory.TrimEnd("\")) {
    Write-Host "Add $InstallDirectory to PATH, open a new terminal, then run ``jevia init``."
  } else {
    Write-Host "Run ``jevia init`` to get started."
  }
} finally {
  if ($temporaryDestination -and [IO.File]::Exists($temporaryDestination)) {
    Remove-Item -LiteralPath $temporaryDestination -Force
  }
  if (Test-Path -LiteralPath $TemporaryDirectory) {
    Remove-Item -LiteralPath $TemporaryDirectory -Recurse -Force
  }
}
