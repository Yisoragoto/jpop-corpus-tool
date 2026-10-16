<#
.SYNOPSIS
不发版也能试新代码：构建当前代码，拷一份你的库，用这份副本启动新版。

.DESCRIPTION
和装好的那一份互不影响：

* 程序：新构建的 jp-app.exe 拷到副本目录的 bin\ 下运行，不碰安装目录。
  装好的版本可以同时开着，两个窗口并排比。
* 库：每次启动前把 corpus.db 重新拷一份（VACUUM INTO，原库开着也能安全地拷），
  封面、歌手照片、歌词跟着拷，路径改指向副本。新版在副本里做的任何改动——
  迁移结构版本、刮削、编辑、收听记录——都只落在副本里，你的库原样不动。
  词典库（几百 MB）和 Sudachi 词典第一次拷，之后沿用，要刷新加 -FreshDictionaries。
* 设置：WebView2 的数据目录和 LOCALAPPDATA 都指到副本目录，
  所以界面设置、记住的库目录也是各管各的。

两个例外，在副本里也是真的：
* 音频文件不拷，副本直接读原来的文件（只读，播放不会改它）。
* 制卡写的是你真实的 Anki。

.PARAMETER SkipBuild
不重新构建，直接用上次构建好的程序（只想重新拷库、再开一次时用）。

.PARAMETER KeepLibrary
不重新拷库，接着用副本里现有的那份（想保留上一次在副本里做的改动时用）。

.PARAMETER FreshDictionaries
词典库和 Sudachi 词典也重新拷一遍。

.PARAMETER Library
你的库在哪。默认 %LOCALAPPDATA%\JPOP Corpus Tool（装好的版本用的那个）。

.PARAMETER Sandbox
副本放在哪。默认 %LOCALAPPDATA%\JPOP Corpus Tool Sandbox。

.PARAMETER DebugPort
给 WebView2 开远程调试端口（比如 9335），自动化验证界面时用；平时不用给。

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\try-dev.ps1

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\try-dev.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$KeepLibrary,
    [switch]$FreshDictionaries,
    [string]$Library = (Join-Path $env:LOCALAPPDATA 'JPOP Corpus Tool'),
    [string]$Sandbox = (Join-Path $env:LOCALAPPDATA 'JPOP Corpus Tool Sandbox'),
    [int]$DebugPort = 0
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$rust = Join-Path $repo 'rust'
$app = Join-Path $repo 'app'
$built = Join-Path $rust 'target\release\jp-app.exe'
$copier = Join-Path $rust 'target\release\examples\sandbox_copy.exe'
$bin = Join-Path $Sandbox 'bin'
$exe = Join-Path $bin 'jp-app.exe'
$libraryCopy = Join-Path $Sandbox 'library'

function Step($text) { Write-Host ''; Write-Host "== $text" -ForegroundColor Cyan }
function Check($what) {
    if ($LASTEXITCODE -ne 0) { throw "$what 失败（退出码 $LASTEXITCODE）" }
}

# ── 先挡住会伤到真库的情况 ──
if (-not (Test-Path (Join-Path $Library 'corpus.db'))) {
    throw "在 $Library 里找不到 corpus.db。库不在默认位置的话用 -Library 指过去。"
}
$fullLibrary = [System.IO.Path]::GetFullPath($Library).TrimEnd('\')
$fullSandbox = [System.IO.Path]::GetFullPath($Sandbox).TrimEnd('\')
if ($fullSandbox -eq $fullLibrary -or $fullSandbox.StartsWith($fullLibrary + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw "副本目录不能是你的库或它的子目录：$Sandbox"
}

# ── 1. 构建 ──
if (-not $SkipBuild) {
    Step '构建前端'
    Push-Location $app
    try {
        if (-not (Test-Path (Join-Path $app 'node_modules'))) {
            & npm ci --no-audit --no-fund
            Check 'npm ci'
        }
        & npm run build
        Check '前端构建'
    } finally { Pop-Location }

    Step '构建程序（release；第一次要几分钟，之后快得多）'
    Push-Location $rust
    try {
        & cargo build --release -p jp-app --features custom-protocol
        Check 'cargo build jp-app'
        & cargo build --release -p jp-corpus --example sandbox_copy
        Check 'cargo build sandbox_copy'
    } finally { Pop-Location }
}
if (-not (Test-Path $built)) { throw "没有构建好的程序：$built。去掉 -SkipBuild 再跑一次。" }
if (-not (Test-Path $copier)) { throw "没有拷库工具：$copier。去掉 -SkipBuild 再跑一次。" }

# ── 2. 关掉上一次开的副本（只认副本目录里那个 exe，装好的版本不碰） ──
$old = Get-Process jp-app -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
if ($old) {
    Step '关掉上一次开的副本'
    $old | Stop-Process -Force
    $old | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
}

# ── 3. 拷程序 ──
New-Item -ItemType Directory -Force $bin | Out-Null
Copy-Item $built $exe -Force

# ── 4. 拷库 ──
if ($KeepLibrary -and (Test-Path (Join-Path $libraryCopy 'corpus.db'))) {
    Step '沿用副本里现有的库（-KeepLibrary）'
} else {
    Step "拷库：$Library → $libraryCopy"
    $copyArgs = @($Library, $libraryCopy)
    if ($FreshDictionaries) { $copyArgs += '--dictionaries' }
    & $copier @copyArgs
    Check '拷库'
}

# ── 5. 启动 ──
$commit = (& git -C $repo rev-parse --short HEAD)
$dirty = @(& git -C $repo status --porcelain).Count
$label = if ($dirty -gt 0) { "$commit（另有 $dirty 处没提交的改动）" } else { $commit }

Step "启动新版：$label"
$saved = @{}
$vars = @{
    JPOP_CORPUS_HOME          = $libraryCopy
    LOCALAPPDATA              = (Join-Path $Sandbox 'localappdata')
    WEBVIEW2_USER_DATA_FOLDER = (Join-Path $Sandbox 'webview')
}
if ($DebugPort -gt 0) { $vars['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = "--remote-debugging-port=$DebugPort" }
foreach ($name in $vars.Keys) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
    if ($name -ne 'WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS') { New-Item -ItemType Directory -Force $vars[$name] | Out-Null }
    [Environment]::SetEnvironmentVariable($name, $vars[$name], 'Process')
}
try {
    $proc = Start-Process $exe -WorkingDirectory $bin -PassThru
} finally {
    # 只给子进程用，不留在你的终端里
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}

Write-Host ''
Write-Host "已启动（进程 $($proc.Id)）。用的是副本：$libraryCopy" -ForegroundColor Green
Write-Host '你的库和装好的版本都没动。音频读的是原文件；制卡会写进真实的 Anki。'
Write-Host '再跑一次本脚本会关掉这个窗口、重新构建、重新拷库。'
