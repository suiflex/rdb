param(
  [string]$Version = $env:RDB_VERSION,
  [string]$InstallDir = $env:INSTALL_DIR
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Repo = "suiflex/rdb"
$Binary = "rdb.exe"

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Write-Step {
  param([string]$Message)
  Write-Host "==> $Message"
}

function Get-Arch {
  $raw = if ($env:PROCESSOR_ARCHITEW6432) {
    $env:PROCESSOR_ARCHITEW6432
  } else {
    $env:PROCESSOR_ARCHITECTURE
  }

  switch -Regex ($raw) {
    "^(AMD64|x86_64)$" { return "x86_64" }
    "^ARM64$" { return "aarch64" }
    default { throw "unsupported architecture: $raw" }
  }
}

if ([string]::IsNullOrWhiteSpace($Version)) {
  $Version = "latest"
}

$Arch = Get-Arch
$Target = "$Arch-pc-windows-msvc"
$ApiUrl = if ($Version -eq "latest") {
  "https://api.github.com/repos/$Repo/releases/latest"
} else {
  "https://api.github.com/repos/$Repo/releases/tags/$Version"
}

Write-Step "Looking up $Repo release ($Version)"
$Headers = @{
  Accept = "application/vnd.github+json"
  "User-Agent" = "rdb-windows-installer"
}
$Release = Invoke-RestMethod -Uri $ApiUrl -Headers $Headers
$AssetName = "rdb-$Target.zip"
$Asset = $Release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
if (-not $Asset) {
  throw "could not find release asset: $AssetName"
}

if ([string]::IsNullOrWhiteSpace($InstallDir)) {
  $LocalAppData = if ($env:LOCALAPPDATA) {
    $env:LOCALAPPDATA
  } else {
    Join-Path $HOME "AppData\Local"
  }
  $InstallDir = Join-Path $LocalAppData "Programs\RDB\bin"
}

$TmpDir = Join-Path ([IO.Path]::GetTempPath()) "rdb-install-$([Guid]::NewGuid())"
$ArchivePath = Join-Path $TmpDir $AssetName
$ExtractDir = Join-Path $TmpDir "extracted"

New-Item -ItemType Directory -Path $ExtractDir -Force | Out-Null

try {
  Write-Step "Downloading $AssetName"
  Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $ArchivePath -Headers $Headers

  Expand-Archive -Path $ArchivePath -DestinationPath $ExtractDir -Force
  $BinPath = Get-ChildItem -Path $ExtractDir -Filter $Binary -Recurse -File |
    Select-Object -First 1
  if (-not $BinPath) {
    throw "downloaded archive does not contain $Binary"
  }

  New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
  $DestPath = Join-Path $InstallDir $Binary
  Copy-Item -Path $BinPath.FullName -Destination $DestPath -Force
  try {
    Unblock-File -Path $DestPath
  } catch {
    # Older PowerShell hosts may not have attachment-zone data to clear.
  }

  Write-Step "Installed to $DestPath"

  $PathEntries = ($env:PATH -split ";") | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
  $OnPath = $PathEntries | Where-Object { $_.TrimEnd("\") -ieq $InstallDir.TrimEnd("\") }
  if (-not $OnPath) {
    Write-Warning "$InstallDir is not on PATH yet"
    Write-Warning "Add it to PATH, then open a new terminal to run rdb"
  }
} finally {
  Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
