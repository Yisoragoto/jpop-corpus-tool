<#
.SYNOPSIS
发版前的自检。构建完、打标签之前跑一次。

.DESCRIPTION
守的是「发出去的那一份能不能装、能不能打开」。v0.2.1 就是没守这一条：
安装包装好了、双击没反应，因为找库的逻辑在**干净环境**下才会走到那条错路，
而开发树里到处都是 corpus.db，本机怎么跑都碰不到。

它做五件事：

1. 三处版本号一致（tauri.conf.json / jp-app 的 Cargo.toml / app 的 package.json）；
2. 安装包确实构建出来了，而且文件名里的版本号对得上；
3. 把 jp-app.exe 单独放进一个空目录，用**干净的环境变量**跑起来：
   没有 JPOP_CORPUS_HOME、假的 LOCALAPPDATA、独立的 WebView2 配置目录；
4. 从真实界面问一遍 health 和分词词典状态，断言：
   页面加载出来了、左侧导航在、库建在了假的 LOCALAPPDATA 下、是个空库；
5. 收拾干净。

**它不测安装程序本身**（静默安装会动注册表和开始菜单，可能踩到你装着的那一份）。
测的是「这个 exe 在一台没有这个项目的机器上能不能起来」——0.2.1 那个坑就在这儿。

.PARAMETER Version
期望的版本号，比如 0.2.7。不给就从 tauri.conf.json 读。

.PARAMETER Port
WebView2 远程调试端口，默认 9412。本机同时跑着别的实例时换一个。

.EXAMPLE
  pwsh -File scripts/release-check.ps1
#>
[CmdletBinding()]
param(
    [string]$Version,
    [int]$Port = 9412
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$fail = @()

function Step($text) { Write-Host "`n── $text" -ForegroundColor Cyan }
function Good($text) { Write-Host "  [ok] $text" -ForegroundColor Green }
function Bad($text) { Write-Host "  [!!] $text" -ForegroundColor Red; $script:fail += $text }

# ── 1. 版本号 ──
Step '版本号三处一致'
$conf = Join-Path $repo 'rust/crates/jp-app/tauri.conf.json'
$tauriVersion = (Get-Content $conf -Raw | ConvertFrom-Json).version
$cargoVersion = (Select-String -Path (Join-Path $repo 'rust/crates/jp-app/Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$pkgVersion = (Get-Content (Join-Path $repo 'app/package.json') -Raw | ConvertFrom-Json).version
if (-not $Version) { $Version = $tauriVersion }
Write-Host "  tauri.conf.json=$tauriVersion  Cargo.toml=$cargoVersion  package.json=$pkgVersion"
if ($tauriVersion -eq $cargoVersion -and $cargoVersion -eq $pkgVersion -and $tauriVersion -eq $Version) {
    Good "都是 $Version"
} else {
    Bad "版本号对不上（期望 $Version）"
}

# ── 2. 安装包 ──
Step '安装包在不在'
$bundle = Join-Path $repo 'rust/target/release/bundle'
$nsis = Join-Path $bundle "nsis/JPOP Corpus Tool_${Version}_x64-setup.exe"
$msi = Join-Path $bundle "msi/JPOP Corpus Tool_${Version}_x64_en-US.msi"
foreach ($pkg in @($nsis, $msi)) {
    if (Test-Path $pkg) {
        $size = (Get-Item $pkg).Length
        $hash = (Get-FileHash $pkg -Algorithm SHA256).Hash.ToLower()
        Good ("{0}  {1:N0} 字节  sha256 {2}" -f (Split-Path $pkg -Leaf), $size, $hash)
    } else {
        Bad "没有 $(Split-Path $pkg -Leaf)——先跑 npm run app:build"
    }
}

$exe = Join-Path $repo 'rust/target/release/jp-app.exe'
if (-not (Test-Path $exe)) { Bad "没有 jp-app.exe" }
if ($fail.Count -gt 0) {
    Write-Host "`n检查没通过：`n  - $($fail -join "`n  - ")" -ForegroundColor Red
    exit 1
}

# ── 3+4. 干净环境里跑一次 ──
Step '空目录 + 干净环境里启动'
$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) "jpop-release-check-$PID"
Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$sandbox/app" | Out-Null
New-Item -ItemType Directory -Force "$sandbox/localappdata" | Out-Null
Copy-Item $exe "$sandbox/app/jp-app.exe"

$proc = $null
try {
    # **干净**：没有 JPOP_CORPUS_HOME、LOCALAPPDATA 指到沙箱、WebView2 用自己的配置目录
    # （不单独给的话会去复用本机那个正在跑的实例，调试端口根本不开）
    $env:JPOP_CORPUS_HOME = $null
    Remove-Item Env:JPOP_CORPUS_HOME -ErrorAction SilentlyContinue
    $env:LOCALAPPDATA = "$sandbox/localappdata"
    $env:WEBVIEW2_USER_DATA_FOLDER = "$sandbox/webview2"
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"

    $proc = Start-Process -FilePath "$sandbox/app/jp-app.exe" -WorkingDirectory "$sandbox/app" -PassThru
    Start-Sleep -Seconds 8
    if ($proc.HasExited) {
        Bad "启动就退了，退出码 $($proc.ExitCode)——这正是 v0.2.1 那个「双击没反应」"
        throw '启动失败'
    }
    Good "起来了，pid $($proc.Id)"

    Step '问一遍界面和后端'
    $json = & node (Join-Path $PSScriptRoot 'smoke.mjs') $Port
    if ($LASTEXITCODE -ne 0) { Bad "smoke.mjs 没跑通"; throw 'smoke 失败' }
    $r = $json | ConvertFrom-Json

    if ($r.href -like 'http*') { Good "页面：$($r.href)" } else { Bad "页面没加载出来：$($r.href)" }
    if ($r.navButtons.Count -ge 8) { Good "左侧导航 $($r.navButtons.Count) 项" } else { Bad "导航只有 $($r.navButtons.Count) 项，界面没渲染全" }
    if ($r.health.dbPath -like "*localappdata*") { Good "空库建在 $($r.health.dbPath)" } else { Bad "库建到了别处：$($r.health.dbPath)" }
    if ($r.health.tracks -eq 0) { Good "新库 0 首歌" } else { Bad "新库里居然有 $($r.health.tracks) 首" }
    # 词典是按需下载的，干净环境里本来就没有——这里只是把状态打出来，不当失败
    Write-Host "  （分词词典：ready=$($r.dict.ready) source=$($r.dict.source)，干净环境里没有是对的）"
} finally {
    if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Seconds 1
    Remove-Item -Recurse -Force $sandbox -ErrorAction SilentlyContinue
}

Write-Host ''
if ($fail.Count -gt 0) {
    Write-Host "检查没通过：`n  - $($fail -join "`n  - ")" -ForegroundColor Red
    exit 1
}
Write-Host "$Version 可以发了。" -ForegroundColor Green
