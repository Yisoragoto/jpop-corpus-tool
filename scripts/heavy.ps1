[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Command,
    [int]$TimeoutSeconds = 3600,
    [string]$WorkingDirectory
)
$ErrorActionPreference = 'Stop'
$repo = [System.IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot)).ToLowerInvariant()
$bytes = [System.Text.Encoding]::UTF8.GetBytes($repo)
$hasher = [System.Security.Cryptography.SHA256]::Create()
try { $hash = (-join ($hasher.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') })).Substring(0, 24) }
finally { $hasher.Dispose() }
$mutex = [System.Threading.Mutex]::new($false, "Local\JPCorpusHeavy-$hash")
$held = $false
try {
    try {
        $held = $mutex.WaitOne(0)
        if (-not $held) {
            Write-Host '[heavy] 排队等待重活租约…'
            $held = $mutex.WaitOne([TimeSpan]::FromSeconds($TimeoutSeconds))
        }
    } catch [System.Threading.AbandonedMutexException] {
        # 持有者被杀时内核会释放；接手者不能被一个永远不会删的锁文件困住。
        $held = $true
        Write-Host '[heavy] 上一个持有者已退出，接手租约'
    }
    if (-not $held) { throw "重活租约等待超过 $TimeoutSeconds 秒" }
    if (-not $WorkingDirectory) {
        $WorkingDirectory = if ($Command -match '^\s*cargo\b') { Join-Path $repo 'rust' }
            elseif ($Command -match '^\s*npm\b') { Join-Path $repo 'app' } else { $repo }
    }
    Push-Location -LiteralPath $WorkingDirectory
    try {
        $shellPath = (Get-Process -Id $PID).Path
        & $shellPath -NoProfile -Command $Command
        $code = $LASTEXITCODE
    } finally { Pop-Location }
} finally {
    if ($held) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
exit $code
