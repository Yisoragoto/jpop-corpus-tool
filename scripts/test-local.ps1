# Full local run: also runs the tests marked #[ignore] for external preconditions
# (audio output device, the author's corpus.db, Sudachi dictionary, benchmark files).
# CI cannot provide these, so they are ignored there; a developer machine usually can.
# Network tests are skipped: they hit public services and must be run on purpose.
# A test whose precondition is missing fails with a message naming it.
[CmdletBinding()]
param([string[]]$Package)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$network = @(
    'public_sources_attach_real_units_to_an_isolated_library',
    'scraping_one_track_end_to_end',
    'a_batch_job_runs_in_the_background_and_reports_progress',
    'scraping_an_artist_end_to_end'
)
$scope = if ($Package) { $Package | ForEach-Object { '-p'; $_ } } else { @('--workspace') }
$skip = $network | ForEach-Object { '--skip'; $_ }
Push-Location "$repo/rust"
try {
    & cargo test @scope --no-fail-fast -- --include-ignored @skip
    exit $LASTEXITCODE
} finally { Pop-Location }
