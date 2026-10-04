Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepositoryRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Installer = Join-Path $RepositoryRoot "website\public\install.ps1"
$TestRoot = Join-Path ([IO.Path]::GetTempPath()) "jevia-installer-test-$([Guid]::NewGuid().ToString('N'))"
$global:JeviaFixtureDirectory = Join-Path $TestRoot "fixtures"
$InstallDirectory = Join-Path $TestRoot "bin"
$Version = "9.8.7"
$AssetName = "jevia-v$Version-x86_64-pc-windows-msvc.exe"
$AssetPath = Join-Path $global:JeviaFixtureDirectory $AssetName

function global:Invoke-WebRequest {
  param(
    [Parameter(Mandatory = $true)] [Uri]$Uri,
    [Parameter(Mandatory = $true)] [string]$OutFile,
    [switch]$UseBasicParsing
  )

  $fixture = Join-Path $global:JeviaFixtureDirectory ([IO.Path]::GetFileName($Uri.AbsolutePath))
  Copy-Item -LiteralPath $fixture -Destination $OutFile -Force
}

try {
  New-Item -ItemType Directory -Path $global:JeviaFixtureDirectory -Force | Out-Null
  [IO.File]::WriteAllBytes($AssetPath, [byte[]](1..64))
  $Digest = (Get-FileHash -LiteralPath $AssetPath -Algorithm SHA256).Hash.ToLowerInvariant()
  [IO.File]::WriteAllText("$AssetPath.sha256", "$Digest  $AssetName`n")

  $env:JEVIA_VERSION = $Version
  $env:JEVIA_INSTALL_DIR = $InstallDirectory
  $env:JEVIA_DOWNLOAD_BASE_URL = "https://downloads.example.test/v$Version"
  & $Installer

  $InstalledBinary = Join-Path $InstallDirectory "jevia.exe"
  if (-not (Test-Path -LiteralPath $InstalledBinary)) {
    throw "installer did not create jevia.exe"
  }
  $InstalledDigest = (Get-FileHash -LiteralPath $InstalledBinary -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($InstalledDigest -ne $Digest) {
    throw "installed binary does not match the verified fixture"
  }

  # Replacement still works and leaves no temporary executable behind.
  [IO.File]::WriteAllText($InstalledBinary, "old fixture")
  & $Installer
  if ((Get-FileHash -LiteralPath $InstalledBinary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Digest) {
    throw "installer did not replace the existing regular file"
  }
  if (Get-ChildItem -LiteralPath $InstallDirectory -Force | Where-Object Name -Like ".jevia.install.*") {
    throw "installer left a temporary executable behind"
  }

  # A directory/junction destination must never receive a nested executable.
  foreach ($Kind in @("directory", "junction")) {
    $CollisionDirectory = Join-Path $TestRoot $Kind
    New-Item -ItemType Directory -Path $CollisionDirectory | Out-Null
    $Collision = Join-Path $CollisionDirectory "jevia.exe"
    if ($Kind -eq "junction") {
      $Target = Join-Path $CollisionDirectory "target"
      New-Item -ItemType Directory -Path $Target | Out-Null
      New-Item -ItemType Junction -Path $Collision -Target $Target | Out-Null
    } else {
      New-Item -ItemType Directory -Path $Collision | Out-Null
    }
    [IO.File]::WriteAllText((Join-Path $Collision "keep"), "preserve me")
    $env:JEVIA_INSTALL_DIR = $CollisionDirectory
    $Rejected = $false
    try { & $Installer } catch {
      if ($_.Exception.Message -match "install destination is not a regular file") { $Rejected = $true }
      else { throw }
    }
    if (-not $Rejected) { throw "installer accepted a $Kind destination" }
    $Children = @(Get-ChildItem -LiteralPath $Collision -Force)
    if ($Children.Count -ne 1 -or $Children[0].Name -ne "keep") {
      throw "installer changed the collision destination"
    }
  }
  $env:JEVIA_INSTALL_DIR = $InstallDirectory

  [IO.File]::WriteAllText("$AssetPath.sha256", "$('0' * 64)  $AssetName`n")
  $Rejected = $false
  try {
    & $Installer
  } catch {
    if ($_.Exception.Message -match "checksum verification failed") {
      $Rejected = $true
    } else {
      throw
    }
  }
  if (-not $Rejected) {
    throw "installer accepted a release binary with the wrong checksum"
  }
  if ((Get-FileHash -LiteralPath $InstalledBinary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Digest) {
    throw "failed upgrade modified the existing binary"
  }
} finally {
  Remove-Item Env:JEVIA_VERSION -ErrorAction SilentlyContinue
  Remove-Item Env:JEVIA_INSTALL_DIR -ErrorAction SilentlyContinue
  Remove-Item Env:JEVIA_DOWNLOAD_BASE_URL -ErrorAction SilentlyContinue
  Remove-Item Function:\Invoke-WebRequest -ErrorAction SilentlyContinue
  Remove-Variable JeviaFixtureDirectory -Scope Global -ErrorAction SilentlyContinue
  if (Test-Path -LiteralPath $TestRoot) {
    Remove-Item -LiteralPath $TestRoot -Recurse -Force
  }
}
