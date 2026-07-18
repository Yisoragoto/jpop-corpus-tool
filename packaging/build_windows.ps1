param(
    [string]$Version = "0.1.0",
    [string]$FfmpegPath = "",
    [string]$InnoSetupPath = "",
    [switch]$SkipFreeze,
    [switch]$SkipInstaller,
    [switch]$KeepBuild
)

$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$VenvPython = Join-Path $Root ".venv\Scripts\python.exe"
$DistDir = Join-Path $Root "dist\JpopCorpusTool"
$ZipPath = Join-Path $Root "dist\JpopCorpusTool-$Version-windows-portable.zip"
$SetupPath = Join-Path $Root "dist\JpopCorpusTool-$Version-windows-setup.exe"

Set-Location $Root

if (-not (Test-Path $VenvPython)) {
    Write-Host "Creating .venv ..."
    $py = Get-Command py -ErrorAction SilentlyContinue
    if ($py) {
        & py -3.12 -m venv .venv
    } else {
        & python -m venv .venv
    }
}

Write-Host "Installing runtime dependencies ..."
& $VenvPython -m pip install --upgrade pip
& $VenvPython -m pip install -r requirements.txt

Write-Host "Installing packaging dependencies ..."
& $VenvPython -m pip install -r requirements-dev.txt

Write-Host "Building Windows portable app ..."
if ($SkipFreeze) {
    Write-Host "Reusing existing PyInstaller output ..."
} else {
    & $VenvPython -m PyInstaller --noconfirm --clean "packaging\JpopCorpusTool.spec"
}

if (-not (Test-Path $DistDir)) {
    throw "PyInstaller output not found: $DistDir"
}

foreach ($dir in @(
    "raw\audio",
    "raw\lyrics_lrc",
    "metadata",
    "processed",
    "output",
    "backups"
)) {
    New-Item -ItemType Directory -Path (Join-Path $DistDir $dir) -Force | Out-Null
}

Copy-Item README.md (Join-Path $DistDir "README.md") -Force
Copy-Item LICENSE (Join-Path $DistDir "LICENSE") -Force
Copy-Item THIRD_PARTY_NOTICES.md (Join-Path $DistDir "THIRD_PARTY_NOTICES.md") -Force

function Copy-CleanDirectory([string]$Source, [string]$Name) {
    $destination = Join-Path $DistDir $Name
    $distFull = [IO.Path]::GetFullPath($DistDir).TrimEnd('\') + '\'
    $destinationFull = [IO.Path]::GetFullPath($destination)
    if (-not $destinationFull.StartsWith($distFull, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to replace directory outside the release folder: $destinationFull"
    }
    if (Test-Path $destinationFull) {
        Remove-Item -LiteralPath $destinationFull -Recurse -Force
    }
    Copy-Item $Source $destinationFull -Recurse -Force
}

Copy-CleanDirectory "examples" "examples"
Copy-CleanDirectory "scripts" "scripts"
Copy-CleanDirectory "assets" "assets"

$BundledDb = Join-Path $DistDir "corpus.db"
if (Test-Path $BundledDb) {
    Remove-Item $BundledDb -Force
}

if (-not $FfmpegPath) {
    $cmd = Get-Command ffmpeg -ErrorAction SilentlyContinue
    if ($cmd) {
        $FfmpegPath = $cmd.Source
    }
}

if ($FfmpegPath -and (Test-Path $FfmpegPath)) {
    Copy-Item $FfmpegPath (Join-Path $DistDir "ffmpeg.exe") -Force
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "SilentlyContinue"
    $ffmpegVersion = (& $FfmpegPath -version 2>&1 | Out-String).Trim()
    $ffmpegLicense = (& $FfmpegPath -L 2>&1 | Out-String).Trim()
    $ErrorActionPreference = $previousErrorAction
    @(
        "This release bundles an independent FFmpeg executable.",
        "Build and license information reported by that executable:",
        "",
        $ffmpegVersion,
        "",
        $ffmpegLicense
    ) | Set-Content (Join-Path $DistDir "FFMPEG_BUILD_INFO.txt") -Encoding UTF8
    Write-Host "Bundled ffmpeg: $FfmpegPath"
} else {
    Write-Warning "ffmpeg.exe was not bundled. Audio clipping will be disabled unless users install ffmpeg."
}

if (Test-Path $ZipPath) {
    Remove-Item $ZipPath -Force
}

Write-Host "Creating portable zip ..."
Compress-Archive -Path (Join-Path $DistDir "*") -DestinationPath $ZipPath -Force

if (-not $SkipInstaller) {
    if (-not $InnoSetupPath) {
        $iscc = Get-Command ISCC.exe -ErrorAction SilentlyContinue
        if ($iscc) {
            $InnoSetupPath = $iscc.Source
        } else {
            $isccCandidates = @(
                (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
                (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
                (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe")
            )
            foreach ($candidate in $isccCandidates) {
                if (Test-Path $candidate) {
                    $InnoSetupPath = $candidate
                    break
                }
            }
        }
    }

    if (-not $InnoSetupPath -or -not (Test-Path $InnoSetupPath)) {
        throw "Inno Setup 6 was not found. Install it or pass -InnoSetupPath; use -SkipInstaller only for a portable-only build."
    }

    Write-Host "Creating Windows installer ..."
    & $InnoSetupPath "/DMyAppVersion=$Version" "packaging\JpopCorpusTool.iss"
    if (-not (Test-Path $SetupPath)) {
        throw "Installer output not found: $SetupPath"
    }
}

if (-not $KeepBuild) {
    $BuildDir = Join-Path $Root "build"
    if (Test-Path $BuildDir) {
        Write-Host "Removing intermediate build directory ..."
        Remove-Item -LiteralPath $BuildDir -Recurse -Force
    }
}

Write-Host ""
Write-Host "Portable: $ZipPath"
if (-not $SkipInstaller) {
    Write-Host "Installer: $SetupPath"
}
