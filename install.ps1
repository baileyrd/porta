# porta bootstrap installer (Windows, native — no WSL required).
#
#   irm https://raw.githubusercontent.com/baileyrd/porta/main/install.ps1 | iex
#
# Run from PowerShell. You do NOT need to run as Administrator — everything
# here is scoped to the current user:
#   - installs the `porta.exe` binary into $PortaHome\bin (default
#     %LOCALAPPDATA%\porta\bin)
#   - if no prebuilt release is available for this platform yet, builds
#     porta from source, installing a user-local Rust toolchain via rustup
#     first if one isn't already on PATH
#   - runs `porta init` to add porta's bin dir to your user PATH
#     (HKCU\Environment — never the machine-wide PATH)
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
    $asset = "porta-windows-$Arch.zip"
    if ($Tag -eq "latest") {
        $url = "https://github.com/$Repo/releases/latest/download/$asset"
    } else {
        $url = "https://github.com/$Repo/releases/download/$Tag/$asset"
    }

    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $tmp -Force | Out-Null
    try {
        $archivePath = Join-Path $tmp $asset
        Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing -ErrorAction Stop

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
    Ensure-RustToolchain
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        throw "git is required to build porta from source (install Git for Windows first)"
    }

    $srcDir = Join-Path $PortaHome "src\porta"
    if (Test-Path $srcDir) {
        Remove-Item -Recurse -Force $srcDir
    }
    New-Item -ItemType Directory -Path (Split-Path $srcDir -Parent) -Force | Out-Null

    Write-Log "cloning $Repo..."
    if ($Tag -eq "latest") {
        git clone --depth 1 "https://github.com/$Repo" $srcDir
    } else {
        git clone --depth 1 --branch $Tag "https://github.com/$Repo" $srcDir
    }
    if ($LASTEXITCODE -ne 0) {
        throw "git clone failed"
    }

    Write-Log "building porta (this can take a minute the first time)..."
    Push-Location $srcDir
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed"
        }
    } finally {
        Pop-Location
    }

    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    Copy-Item -Path (Join-Path $srcDir "target\release\porta.exe") -Destination (Join-Path $BinDir "porta.exe") -Force
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
