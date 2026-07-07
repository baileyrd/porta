# porta bootstrap installer (Windows, native — no WSL required).
#
#   irm https://raw.githubusercontent.com/baileyrd/porta/main/install.ps1 | iex
#
# Run from PowerShell. You do NOT need to run as Administrator — everything
# here is scoped to the current user:
#   - installs the `porta.exe` binary into $PortaHome\bin (default
#     %LOCALAPPDATA%\porta\bin)
#   - if no prebuilt release is available for this platform yet, builds
#     porta from a source ZIP (no git required), installing a user-local
#     Rust toolchain via rustup first if one isn't already on PATH
#   - runs `porta init` to add porta's bin dir to your user PATH
#     (HKCU\Environment — never the machine-wide PATH)
#
# Host requirements are the floor Windows itself provides: PowerShell with
# Invoke-WebRequest and Expand-Archive. Nothing else is assumed present.
#
# Optional: pass a specific release tag instead of the latest:
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/baileyrd/porta/main/install.ps1))) v0.2.0

param(
    [string]$Version = "latest"
)

$ErrorActionPreference = "Stop"

$Repo = "baileyrd/porta"
$PortaHome = if ($env:PORTA_HOME) { $env:PORTA_HOME } else { Join-Path $env:LOCALAPPDATA "porta" }
$BinDir = Join-Path $PortaHome "bin"

function Write-Log([string]$Message) {
    Write-Host "porta-install: $Message"
}

function Get-PortaArch {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch -Regex ($arch) {
        "ARM64" { return "aarch64" }
        default { return "x86_64" }
    }
}

function Try-InstallPrebuilt([string]$Arch, [string]$Tag) {
    if ($Tag -eq "latest") {
        $base = "https://github.com/$Repo/releases/latest/download"
    } else {
        $base = "https://github.com/$Repo/releases/download/$Tag"
    }

    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null
    try {
        # Preferred asset: the raw binary — nothing to extract.
        try {
            $rawPath = Join-Path $tmp "porta.exe"
            Invoke-WebRequest -Uri "$base/porta-windows-$Arch.exe" -OutFile $rawPath -UseBasicParsing -ErrorAction Stop
            Write-Log "found prebuilt release binary (porta-windows-$Arch.exe)"
            New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
            Copy-Item -Path $rawPath -Destination (Join-Path $BinDir "porta.exe") -Force
            return $true
        } catch {
            # fall through to the zip asset shape
        }

        $asset = "porta-windows-$Arch.zip"
        $archivePath = Join-Path $tmp $asset
        Invoke-WebRequest -Uri "$base/$asset" -OutFile $archivePath -UseBasicParsing -ErrorAction Stop

        Write-Log "found prebuilt release ($asset), extracting..."
        New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
        Expand-Archive -LiteralPath $archivePath -DestinationPath $tmp -Force
        Copy-Item -Path (Join-Path $tmp "porta.exe") -Destination (Join-Path $BinDir "porta.exe") -Force
        return $true
    } catch {
        return $false
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

function Ensure-RustToolchain {
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        return
    }
    Write-Log "no Rust toolchain found; installing one for your user via rustup (no admin needed)..."

    $rustupInit = Join-Path ([System.IO.Path]::GetTempPath()) "rustup-init.exe"
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit -UseBasicParsing
    & $rustupInit -y --default-host x86_64-pc-windows-msvc --no-modify-path
    Remove-Item -Force $rustupInit -ErrorAction SilentlyContinue

    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    $env:Path = "$cargoBin;$env:Path"
}

function Build-FromSource([string]$Tag) {
    # Builds from a source ZIP — no git required on the host.
    Ensure-RustToolchain

    if ($Tag -eq "latest") {
        $srcUrl = "https://codeload.github.com/$Repo/zip/refs/heads/main"
    } else {
        $srcUrl = "https://codeload.github.com/$Repo/zip/refs/tags/$Tag"
    }

    $srcDir = Join-Path $PortaHome "src"
    if (Test-Path $srcDir) {
        Remove-Item -Recurse -Force $srcDir
    }
    New-Item -ItemType Directory -Path $srcDir -Force | Out-Null

    Write-Log "downloading porta source archive ($srcUrl)..."
    $srcZip = Join-Path $srcDir "porta-src.zip"
    Invoke-WebRequest -Uri $srcUrl -OutFile $srcZip -UseBasicParsing -ErrorAction Stop
    Expand-Archive -LiteralPath $srcZip -DestinationPath $srcDir -Force
    Remove-Item -Force $srcZip

    # The archive nests everything under porta-<ref>\ — find that directory.
    $srcRoot = Get-ChildItem -Path $srcDir -Directory | Select-Object -First 1
    if (-not $srcRoot) {
        throw "source archive extraction produced no directory"
    }

    Write-Log "building porta (this can take a minute the first time)..."
    Push-Location $srcRoot.FullName
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed"
        }
    } finally {
        Pop-Location
    }

    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    Copy-Item -Path (Join-Path $srcRoot.FullName "target\release\porta.exe") -Destination (Join-Path $BinDir "porta.exe") -Force
    Remove-Item -Recurse -Force $srcDir -ErrorAction SilentlyContinue
}

function Main {
    $arch = Get-PortaArch
    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null

    $prebuilt = Try-InstallPrebuilt -Arch $arch -Tag $Version
    if (-not $prebuilt) {
        Write-Log "no prebuilt binary for windows-$arch (or release '$Version' not found); building from source instead"
        Build-FromSource -Tag $Version
    }

    Write-Log "porta installed at $BinDir\porta.exe"

    $portaExe = Join-Path $BinDir "porta.exe"
    if ($env:PORTA_SKIP_AI -eq "1") {
        & $portaExe init
    } else {
        & $portaExe init --with-ai
    }

    Write-Log "done. Open a new terminal to pick up the updated PATH."
}

Main
