$ErrorActionPreference = 'Stop'
$script = Join-Path (Split-Path -Parent $PSScriptRoot) 'check-release-policy.ps1'
$cases = @(
    @{ Name='1 main debug'; Event='push'; Ref='refs/heads/main'; Channel='debug'; Exit=0 },
    @{ Name='1 main stable 拒绝'; Event='push'; Ref='refs/heads/main'; Channel='stable'; Exit=1 },
    @{ Name='2 分支只检查'; Event='push'; Ref='refs/heads/topic'; Channel='debug'; Exit=0; Build='false'; Publish='false' },
    @{ Name='2 PR 发包拒绝'; Event='pull_request'; Ref='refs/pull/1/merge'; Channel='debug'; Exit=1; Build='true' },
    @{ Name='3 v tag stable 拒绝'; Event='push'; Ref='refs/tags/v0.3.2'; Channel='stable'; Exit=1 },
    @{ Name='3 v tag debug 也不出包'; Event='push'; Ref='refs/tags/v0.3.2'; Channel='debug'; Exit=1 },
    @{ Name='4 手动 stable'; Event='workflow_dispatch'; Ref='refs/heads/main'; Channel='stable'; Tag='v0.3.2'; Exit=0; Prerelease='false'; MakeLatest='true' },
    @{ Name='4 stable 不能预发布'; Event='workflow_dispatch'; Ref='refs/heads/main'; Channel='stable'; Tag='v0.3.2'; Exit=1; Prerelease='true' },
    @{ Name='5 手动 debug'; Event='workflow_dispatch'; Ref='refs/heads/main'; Channel='debug'; Exit=0; Prerelease='true'; MakeLatest='false' },
    @{ Name='5 debug 不能 Latest'; Event='workflow_dispatch'; Ref='refs/heads/main'; Channel='debug'; Exit=1; MakeLatest='true' }
)
foreach ($case in $cases) {
    $params = @('-NoProfile', '-File', $script, '-EventName', $case.Event, '-Ref', $case.Ref, '-Channel', $case.Channel)
    foreach ($key in @('Tag', 'Build', 'Publish', 'Prerelease', 'MakeLatest')) { if ($case.ContainsKey($key)) { $params += "-$key", $case[$key] } }
    $result = & (Get-Process -Id $PID).Path @params 2>&1
    if ($LASTEXITCODE -ne $case.Exit) { throw "$($case.Name): 期望 $($case.Exit) 实际 $LASTEXITCODE`n$result" }
    Write-Output "[ok] $($case.Name)：exit=$LASTEXITCODE"
}
# 最后一项是预期拒绝；不能把子进程的 1 泄漏成整个测试脚本的失败退出码。
exit 0
