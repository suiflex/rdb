$ErrorActionPreference = 'Stop'

$Repo = 'suiflex/rdb'
$Binary = 'storix.exe'
$Version = if ($env:STORIX_VERSION) { $env:STORIX_VERSION } else { 'latest' }
$Arch = switch ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture) {
    'X64' { 'x86_64' }
    'Arm64' { 'aarch64' }
    default { throw "Unsupported architecture: $($_.ToString())" }
}

$ApiUrl = if ($Version -eq 'latest') {
    "https://api.github.com/repos/$Repo/releases/latest"
} else {
    "https://api.github.com/repos/$Repo/releases/tags/$Version"
}

Write-Host "==> Looking up $Repo release ($Version)"
$Release = Invoke-RestMethod -Uri $ApiUrl
if (-not $Release.assets) {
    throw 'No downloadable assets found in release metadata.'
}

$Patterns = @(
    "$Arch.*windows",
    "windows.*$Arch",
    "$Arch-pc-windows-msvc",
    "$Arch-pc-windows-gnu"
)

$Asset = $null
foreach ($Pattern in $Patterns) {
    $Asset = $Release.assets | Where-Object {
        $_.browser_download_url -match $Pattern -and $_.name -match '\.(zip|msi|exe)$'
    } | Select-Object -First 1
    if ($Asset) { break }
}

if (-not $Asset) {
    throw "Could not find a Windows asset for $Arch in release $Version."
}

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("storix-install-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $TmpDir | Out-Null
try {
    $ArchivePath = Join-Path $TmpDir $Asset.name
    Write-Host "==> Downloading $($Asset.name)"
    Invoke-WebRequest -Uri $Asset.browser_download_url -OutFile $ArchivePath

    if ($env:INSTALL_DIR) {
        $InstallDir = $env:INSTALL_DIR
    } else {
        $InstallDir = Join-Path $env:USERPROFILE 'bin'
    }
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

    switch -Regex ($Asset.name) {
        '\.zip$' {
            $ExtractDir = Join-Path $TmpDir 'extract'
            Expand-Archive -Path $ArchivePath -DestinationPath $ExtractDir -Force
            $BinaryPath = Get-ChildItem -Path $ExtractDir -Recurse -File | Where-Object { $_.Name -ieq $Binary } | Select-Object -First 1
            if (-not $BinaryPath) {
                throw "Downloaded archive does not contain $Binary."
            }
            Copy-Item -Path $BinaryPath.FullName -Destination (Join-Path $InstallDir $Binary) -Force
        }
        '\.msi$' {
            Write-Host '==> Launching MSI installer'
            Start-Process msiexec.exe -ArgumentList @('/i', $ArchivePath) -Wait
            return
        }
        '\.exe$' {
            Write-Host '==> Launching EXE installer'
            Start-Process -FilePath $ArchivePath -Wait
            return
        }
        default {
            throw "Unsupported asset format: $($Asset.name)"
        }
    }

    $UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $NeedsPath = -not ($UserPath -split ';' | Where-Object { $_.TrimEnd('\\') -ieq $InstallDir.TrimEnd('\\') })
    if ($NeedsPath) {
        $NewPath = if ([string]::IsNullOrWhiteSpace($UserPath)) { $InstallDir } else { "$UserPath;$InstallDir" }
        [Environment]::SetEnvironmentVariable('Path', $NewPath, 'User')
        Write-Warning "$InstallDir was added to your user PATH. Open a new terminal to use storix."
    }

    Write-Host "==> Installed to $(Join-Path $InstallDir $Binary)"
}
finally {
    Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
}
