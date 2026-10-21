[CmdletBinding()]
param(
    [string]$EventName = $env:GITHUB_EVENT_NAME,
    [string]$Ref = $env:GITHUB_REF,
    [string]$Channel = $env:INPUT_CHANNEL,
    [string]$Tag = $env:RELEASE_TAG,
    [string]$Publish = $env:PUBLISH_RELEASE,
    [string]$Build = $env:BUILD_PACKAGES,
    [string]$Prerelease = $env:RELEASE_PRERELEASE,
    [string]$MakeLatest = $env:MAKE_LATEST,
    [string]$OutputPath = $env:GITHUB_OUTPUT
)
$ErrorActionPreference = 'Stop'
function Denied($rule, $expected, $actual) {
    Write-Output "[拒绝] 命中规则 $rule / 期望：$expected / 实际：$actual"
    exit 1
}
if (-not $Channel) { $Channel = 'debug' }
if ($Channel -notin @('stable', 'debug')) { Denied '轨道' 'stable 或 debug（无 beta）' $Channel }
$expectedBuild = 'true'; $expectedPublish = 'true'
if ($EventName -eq 'push' -and $Ref -eq 'refs/heads/main') {
    $rule = '1 main push'
    if ($Channel -ne 'debug') { Denied $rule 'debug' $Channel }
} elseif ($EventName -eq 'pull_request' -or ($EventName -eq 'push' -and $Ref -like 'refs/heads/*')) {
    $rule = '2 其他分支/PR'
    $expectedBuild = 'false'; $expectedPublish = 'false'
} elseif ($EventName -eq 'push' -and $Ref -like 'refs/tags/v*') {
    Denied '3 tag push' '正式版只能手动 workflow_dispatch 发布；此事件不出包' "$EventName $Ref $Channel"
} elseif ($EventName -eq 'workflow_dispatch') {
    $rule = if ($Channel -eq 'stable') { '4 手动 stable' } else { '5 手动 debug' }
} else {
    Denied '事件' 'main push 或 workflow_dispatch' "$EventName $Ref"
}
$expectedTag = 'debug'; $expectedPre = 'true'; $expectedLatest = 'false'
if ($Channel -eq 'stable' -and $expectedPublish -eq 'true') {
    if (-not $Tag) {
        $repo = Split-Path -Parent $PSScriptRoot
        $version = (Get-Content "$repo/app/package.json" -Raw | ConvertFrom-Json).version
        $Tag = "v$version"
    }
    if ($Tag -notmatch '^v\d+\.\d+\.\d+$') { Denied $rule 'vX.Y.Z 正式 tag' $Tag }
    $expectedTag = $Tag; $expectedPre = 'false'; $expectedLatest = 'true'
}
if (-not $Tag) { $Tag = $expectedTag }
foreach ($pair in @(
    @('tag', $Tag, $expectedTag), @('publish', $Publish, $expectedPublish),
    @('build', $Build, $expectedBuild), @('prerelease', $Prerelease, $expectedPre),
    @('make_latest', $MakeLatest, $expectedLatest)
)) {
    if ($pair[1] -and $pair[1] -ne $pair[2]) { Denied $rule "$($pair[0])=$($pair[2])" "$($pair[0])=$($pair[1])" }
}
$outputs = @("channel=$Channel", "tag=$expectedTag", "build=$expectedBuild", "publish=$expectedPublish", "prerelease=$expectedPre", "make_latest=$expectedLatest")
Write-Output "[通过] 命中规则 $rule / $($outputs -join ' ' )"
if ($OutputPath) { $outputs | Add-Content -LiteralPath $OutputPath -Encoding utf8 }
