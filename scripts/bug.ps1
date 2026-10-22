[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)]
    [ValidateSet('new', 'renumber', 'settle')][string]$Action,
    [Parameter(Position = 1)][string]$Slug,
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)
$ErrorActionPreference = 'Stop'
$docs = Join-Path $Root 'docs'
$bugs = Join-Path $docs 'bugs'
$index = Join-Path $docs 'BUGS.md'
New-Item -ItemType Directory -Force $bugs | Out-Null
if (-not (Test-Path -LiteralPath $index)) {
    throw '缺少 docs/BUGS.md，请先复制仓库里的流程说明。'
}

function Records {
    @(Get-ChildItem -LiteralPath $bugs -File | Where-Object Name -Match '^\d{4}-.+\.md$')
}

if ($Action -eq 'new') {
    if ($Slug -notmatch '^[a-z0-9]+(?:-[a-z0-9]+)*$') { throw 'slug 只用小写英文、数字和连字符。' }
    $last = (Records | ForEach-Object { [int]$_.Name.Substring(0, 4) } | Measure-Object -Maximum).Maximum
    $number = [int]$last + 1
    if ($number -gt 9999) { throw '编号已超过四位，请先调整编号规范。' }
    $id = '{0:D4}' -f $number
    $date = Get-Date -Format 'yyyy-MM-dd'
    $text = @"
# BUG-$id $Slug

- 发现：$date / 版本待填 / 平台待填
- 现象：
- 复现步骤：
- 根因：请填写代码路径、行号和为什么导致故障
- 影响面：
- 修复：☐ 根因修复（提交待填） ☐ 自动化测试（测试文件待填）
- 清理条件：无临时兼容层
"@
    $path = Join-Path $bugs "$id-$Slug.md"
    Set-Content -LiteralPath $path -Value $text -Encoding utf8
    Write-Output $path
} else {
    # 先全部搬到唯一的中间名，避免 0001/0002 对调时覆盖；保留时间让 settle 可重复运行。
    $moves = @()
    $number = 0
    foreach ($file in (Records | Sort-Object LastWriteTimeUtc, Name)) {
        $number++
        $id = '{0:D4}' -f $number
        $temp = Join-Path $bugs ([guid]::NewGuid().ToString() + '.renumber')
        $dest = Join-Path $bugs ($id + $file.Name.Substring(4))
        $moves += [pscustomobject]@{ Temp = $temp; Dest = $dest; Id = $id; Time = $file.LastWriteTimeUtc }
        Move-Item -LiteralPath $file.FullName -Destination $temp
    }
    foreach ($move in $moves) {
        $text = Get-Content -LiteralPath $move.Temp -Raw -Encoding utf8
        $text = $text -replace '(?m)^# BUG-\d{4}\s+', "# BUG-$($move.Id) "
        Set-Content -LiteralPath $move.Temp -Value $text.TrimEnd() -Encoding utf8
        Move-Item -LiteralPath $move.Temp -Destination $move.Dest
        (Get-Item -LiteralPath $move.Dest).LastWriteTimeUtc = $move.Time
    }
}

$rows = @('| 编号 | 标题 | 状态 | 提交 |', '|---|---|---|---|')
foreach ($file in (Records | Sort-Object Name)) {
    $text = Get-Content -LiteralPath $file.FullName -Raw -Encoding utf8
    $title = ([regex]::Match($text, '(?m)^# BUG-\d{4}\s+(.+)$')).Groups[1].Value.Trim().Replace('|', '\|')
    $fix = ([regex]::Match($text, '(?m)^- 修复：(.+)$')).Groups[1].Value
    $done = ([regex]::Matches($fix, '☑|\[[xX]\]')).Count -ge 2
    $state = if ($done) { '已修复' } else { '待修复' }
    $commits = @([regex]::Matches($fix, '\b[0-9a-f]{7,40}\b') | ForEach-Object Value) -join ', '
    if (-not $commits) { $commits = '—' }
    $rows += "| $($file.Name.Substring(0, 4)) | [$title](bugs/$($file.Name)) | $state | $commits |"
}
$body = Get-Content -LiteralPath $index -Raw -Encoding utf8
$block = "<!-- bugs:index -->`n" + ($rows -join "`n") + "`n<!-- /bugs:index -->"
if ($body -notmatch '(?s)<!-- bugs:index -->.*?<!-- /bugs:index -->') { throw 'BUGS.md 缺少索引标记。' }
$body = [regex]::Replace($body, '(?s)<!-- bugs:index -->.*?<!-- /bugs:index -->', [System.Text.RegularExpressions.MatchEvaluator]{ param($match) $block })
Set-Content -LiteralPath $index -Value $body.TrimEnd() -Encoding utf8
