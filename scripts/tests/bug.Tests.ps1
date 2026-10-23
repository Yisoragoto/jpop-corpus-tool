$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ('jp-bugs-' + [guid]::NewGuid())
try {
    New-Item -ItemType Directory -Force "$sandbox/docs" | Out-Null
    Copy-Item -LiteralPath "$repo/docs/BUGS.md" -Destination "$sandbox/docs/BUGS.md"
    & "$repo/scripts/bug.ps1" new first -Root $sandbox | Out-Null
    & "$repo/scripts/bug.ps1" new second -Root $sandbox | Out-Null
    if (-not (Test-Path "$sandbox/docs/bugs/0001-first.md")) { throw '第一次编号错误' }
    if (-not (Test-Path "$sandbox/docs/bugs/0002-second.md")) { throw '第二次编号错误' }
    Rename-Item -LiteralPath "$sandbox/docs/bugs/0002-second.md" -NewName '0001-second.md'
    & "$repo/scripts/bug.ps1" renumber -Root $sandbox
    $files = @(Get-ChildItem "$sandbox/docs/bugs/*.md" | Sort-Object Name)
    if ($files.Count -ne 2 -or $files[1].Name -notlike '0002-*') { throw '重号没有修复' }
    $before = Get-Content "$sandbox/docs/BUGS.md" -Raw
    & "$repo/scripts/bug.ps1" settle -Root $sandbox
    if ((Get-Content "$sandbox/docs/BUGS.md" -Raw) -ne $before) { throw 'settle 不幂等' }
    Write-Output '[ok] new 0001/0002、冲突重排、settle 索引幂等'
} finally {
    $resolved = [System.IO.Path]::GetFullPath($sandbox)
    $tempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot) -and (Split-Path $resolved -Leaf) -like 'jp-bugs-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction SilentlyContinue
    }
}
