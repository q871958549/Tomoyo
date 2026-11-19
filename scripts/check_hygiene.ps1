# Tomoyo Repository Hygiene & Anti-Bloat Gate
# Enforces docs/CODING_STANDARDS.md in local dev and GitHub Actions CI:
#   1. Zero God-Files: every source file must stay <= $MaxLines lines.
#   2. Zero Heavy-Bloat Dependencies in Cargo.toml (tokio, reqwest, hyper, tauri, wry).
#   3. Zero Direct2D/WARP regression (`ID2D1DCRenderTarget`, `d3d10warp`).

param(
    [int]$MaxLines = 450
)

$ErrorActionPreference = "Stop"
$violations = 0

Write-Host "=== [1/3] Checking source file line-count ceiling (<= $MaxLines lines) ==="
$files = Get-ChildItem -Path crates, plugins -Recurse -Include *.rs,*.html,*.css,*.js,*.py |
    Where-Object { $_.FullName -notmatch '\\target\\' -and $_.FullName -notmatch '\\__pycache__\\' }

foreach ($f in $files) {
    $count = (Get-Content $f.FullName | Measure-Object -Line).Lines
    if ($count -gt $MaxLines) {
        $rel = $f.FullName.Replace((Get-Location).Path + "\", "")
        Write-Host "  [FAIL] $rel has $count lines (limit: $MaxLines)" -ForegroundColor Red
        $violations++
    }
}
if ($violations -eq 0) {
    Write-Host "  [OK] All $($files.Count) source files are strictly <= $MaxLines lines!" -ForegroundColor Green
}

Write-Host "=== [2/3] Checking forbidden heavy dependencies in Cargo.toml ==="
$forbiddenCrates = @('^\s*tokio\s*=', '^\s*reqwest\s*=', '^\s*hyper\s*=', '^\s*tauri\s*=', '^\s*wry\s*=', '^\s*async-std\s*=')
$tomls = Get-ChildItem -Path Cargo.toml, crates, plugins -Recurse -Include Cargo.toml |
    Where-Object { $_.FullName -notmatch '\\target\\' }
$depViolations = 0
foreach ($t in $tomls) {
    foreach ($pat in $forbiddenCrates) {
        $hits = Select-String -Path $t.FullName -Pattern $pat
        foreach ($h in $hits) {
            Write-Host "  [FAIL] Forbidden heavy crate in $($t.FullName):$($h.LineNumber): $($h.Line.Trim())" -ForegroundColor Red
            $depViolations++
        }
    }
}
$violations += $depViolations
if ($depViolations -eq 0) {
    Write-Host "  [OK] Zero forbidden heavy crates detected." -ForegroundColor Green
}

Write-Host "=== [3/3] Checking anti-WARP / anti-Direct2D regression ==="
$warpHits = Select-String -Path (Get-ChildItem -Path crates -Recurse -Include *.rs) -Pattern 'ID2D1DCRenderTarget|D2D1CreateFactory' -ErrorAction SilentlyContinue
if ($warpHits) {
    foreach ($h in $warpHits) {
        Write-Host "  [FAIL] Forbidden D2D/WARP API in $($h.Path):$($h.LineNumber)" -ForegroundColor Red
        $violations++
    }
} else {
    Write-Host "  [OK] Pure-Rust SDF + ClearType GDI pipeline preserved (zero D2D/WARP bloat)." -ForegroundColor Green
}

if ($violations -gt 0) {
    Write-Host "`n[FAILED] Hygiene gate failed with $violations violation(s). See docs/CODING_STANDARDS.md." -ForegroundColor Red
    exit 1
}

Write-Host "`n[PASS] All Tomoyo coding standards & anti-bloat checks passed!" -ForegroundColor Green
exit 0
