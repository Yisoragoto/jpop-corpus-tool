$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ('jp-heavy-' + [guid]::NewGuid())
New-Item -ItemType Directory -Force $sandbox | Out-Null
$children = @()
function Start-Heavy($command, $name) {
    $commandFile = Join-Path $sandbox "$name.ps1"
    Set-Content -LiteralPath $commandFile -Value $command -Encoding utf8
    $args = @('-NoProfile','-File',"`"$repo/scripts/heavy.ps1`"",'-TimeoutSeconds','15','-Command',"`"& '$commandFile'`"")
    Start-Process -FilePath (Get-Process -Id $PID).Path -ArgumentList $args -WindowStyle Hidden -PassThru -RedirectStandardOutput "$sandbox/$name.out" -RedirectStandardError "$sandbox/$name.err"
}
function Wait-File($path) {
    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    while (-not (Test-Path -LiteralPath $path)) {
        if ([DateTime]::UtcNow -gt $deadline) { throw "没有出现 $path" }
        Start-Sleep -Milliseconds 50
    }
}
try {
    $one = Start-Heavy "Set-Content '$sandbox/one-start' ([DateTime]::UtcNow.Ticks); Start-Sleep -Seconds 3; Set-Content '$sandbox/one-end' ([DateTime]::UtcNow.Ticks)" 'one'
    $children += $one; Wait-File "$sandbox/one-start"
    $two = Start-Heavy "Set-Content '$sandbox/two-start' ([DateTime]::UtcNow.Ticks)" 'two'
    $children += $two
    if (-not $one.WaitForExit(15000) -or -not $two.WaitForExit(15000)) { throw '排队进程未完成' }
    if ($one.ExitCode -ne 0 -or $two.ExitCode -ne 0) { throw '重活命令退出失败' }
    if ([long](Get-Content "$sandbox/two-start") -lt [long](Get-Content "$sandbox/one-end")) { throw '两个命令重叠了' }
    Write-Output '[ok] 第二个命令排队，直到第一个结束'
    $holder = Start-Heavy "Set-Content '$sandbox/held' yes; Start-Sleep -Seconds 30" 'holder'
    $children += $holder; Wait-File "$sandbox/held"
    $next = Start-Heavy "Set-Content '$sandbox/recovered' yes" 'next'
    $children += $next
    Stop-Process -Id $holder.Id -Force
    if (-not $next.WaitForExit(15000) -or $next.ExitCode -ne 0 -or -not (Test-Path "$sandbox/recovered")) { throw '持有者死亡后没有接手' }
    Write-Output '[ok] 杀死持有者后租约自动失效，后继命令成功'
} finally {
    foreach ($child in $children) { if (-not $child.HasExited) { Stop-Process -Id $child.Id -Force -ErrorAction SilentlyContinue } }
    $resolved = [System.IO.Path]::GetFullPath($sandbox)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot) -and (Split-Path $resolved -Leaf) -like 'jp-heavy-*') { Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue }
}
