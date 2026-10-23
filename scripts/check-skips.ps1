$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$suites = @(
    @('-p','jp-audio','--lib','--test','playback','--test','duration'),
    @('-p','jp-app','--lib','--test','commands','--test','fixture_commands','--test','fixture_flows'),
    @('-p','jp-corpus','--test','real_corpus'),
    @('-p','jp-tokenizer','--test','baseline','--test','lemma_forms')
)
$skips = 0
Push-Location "$repo/rust"
try {
    foreach ($suite in $suites) {
        $lines = & cargo test @suite -- --nocapture 2>&1
        $code = $LASTEXITCODE
        foreach ($line in $lines) {
            Write-Output $line
            if ([string]$line -match '^test (.+) \.\.\. ignored(?:, (.*))?$') {
                Write-Output "[skip] $($Matches[1]): $($Matches[2])"
                $skips++
            }
        }
        if ($code -ne 0) { exit $code }
    }
    if ($skips -eq 0) { throw '没有找到 ignored 原因，skip 日志守卫失效' }
    Write-Output "[skip] 共 $skips 项明确声明了外部前置条件"
} finally { Pop-Location }
