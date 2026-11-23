# Tomoyo Repository Hygiene & Anti-Bloat Gate (7 Gates)
# Enforces docs/CODING_STANDARDS.md in local dev and GitHub Actions CI:
#   1. Zero God-Files: dual-watermark (soft advisory > $WarnLines, hard fail > $MaxLines).
#   2. Zero Heavy-Bloat Dependencies in Cargo.toml (tokio, reqwest, hyper, tauri, wry, async-std).
#   3. Zero Direct2D/WARP regression (`ID2D1DCRenderTarget`, `D2D1CreateFactory`).
#   4. Frontend bundle size ceilings & zero forbidden heavy npm dependencies.
#   5. Rust <-> TypeScript WebView IPC contract parity (`HostMessage` vs `postHost`).
#   6. WebView2 UI JS runtime smoke gate (`scripts/check_webview_ui.mjs`).
#   7. Optional release binary size budget gate (`-CheckBinaries`).

param(
    [int]$MaxLines = 450,
    [int]$WarnLines = 380,
    [switch]$CheckBinaries
)

$ErrorActionPreference = "Stop"
$violations = 0
$totalSteps = if ($CheckBinaries) { 7 } else { 6 }

Write-Host "=== [1/$totalSteps] Checking source file line-count ceiling (warn > $WarnLines, hard fail > $MaxLines) ==="
$files = Get-ChildItem -Path crates, plugins, scripts, frontend\src, frontend\scripts, frontend\tests -Recurse -Include *.rs,*.html,*.css,*.js,*.mjs,*.py,*.ts,*.svelte -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -notmatch '\\target\\' -and $_.FullName -notmatch '\\__pycache__\\' -and $_.FullName -notmatch '\\node_modules\\' -and $_.FullName -notmatch '\\dist\\' }

$softWarnings = @()
foreach ($f in $files) {
    $count = (Get-Content $f.FullName | Measure-Object -Line).Lines
    $rel = $f.FullName.Replace((Get-Location).Path + "\", "")
    if ($count -gt $MaxLines) {
        Write-Host "  [FAIL] $rel has $count lines (limit: $MaxLines)" -ForegroundColor Red
        $violations++
    } elseif ($count -gt $WarnLines) {
        $softWarnings += [PSCustomObject]@{ File = $rel; Lines = $count }
    }
}
if ($violations -eq 0) {
    Write-Host "  [OK] All $($files.Count) source files are strictly <= $MaxLines lines!" -ForegroundColor Green
    if ($softWarnings.Count -gt 0) {
        Write-Host "  [INFO] $($softWarnings.Count) file(s) exceed soft advisory watermark ($WarnLines lines) — consider splitting before growing:" -ForegroundColor Yellow
        $softWarnings | Sort-Object Lines -Descending | ForEach-Object {
            Write-Host "    - $($_.File) ($($_.Lines) lines)" -ForegroundColor DarkYellow
        }
    }
}

Write-Host "=== [2/$totalSteps] Checking forbidden heavy dependencies in Cargo.toml ==="
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

Write-Host "=== [3/$totalSteps] Checking anti-WARP / anti-Direct2D regression ==="
$warpHits = Select-String -Path (Get-ChildItem -Path crates -Recurse -Include *.rs) -Pattern 'ID2D1DCRenderTarget|D2D1CreateFactory' -ErrorAction SilentlyContinue
if ($warpHits) {
    foreach ($h in $warpHits) {
        Write-Host "  [FAIL] Forbidden D2D/WARP API in $($h.Path):$($h.LineNumber)" -ForegroundColor Red
        $violations++
    }
} else {
    Write-Host "  [OK] Pure-Rust SDF + ClearType GDI pipeline preserved (zero D2D/WARP bloat)." -ForegroundColor Green
}

Write-Host "=== [4/$totalSteps] Checking frontend bundle size budgets & forbidden heavy npm packages ==="
$feViolations = 0
$pkgPath = "frontend\package.json"
if (Test-Path $pkgPath) {
    $pkgRaw = Get-Content $pkgPath -Raw
    $forbiddenNpm = @('"react"', '"react-dom"', '"vue"', '"@angular/core"', '"antd"', '"@mui/material"', '"element-plus"', '"lodash"', '"moment"', '"axios"', '"electron"')
    foreach ($pkg in $forbiddenNpm) {
        if ($pkgRaw -match [regex]::Escape($pkg)) {
            Write-Host "  [FAIL] Forbidden heavy npm package $pkg in $pkgPath" -ForegroundColor Red
            $feViolations++
        }
    }
}
$bundleBudgets = [ordered]@{
    "frontend\dist\settings.html"   = 320 * 1024
    "frontend\dist\translator.html" = 150 * 1024
    "frontend\dist\clipboard.html"  = 150 * 1024
    "frontend\dist\ocr.html"        = 150 * 1024
}
$bundleSummary = @()
foreach ($kv in $bundleBudgets.GetEnumerator()) {
    if (Test-Path $kv.Key) {
        $bytes = (Get-Item $kv.Key).Length
        $kb = [math]::Round($bytes / 1024, 1)
        $limKb = [math]::Round($kv.Value / 1024, 0)
        if ($bytes -gt $kv.Value) {
            Write-Host "  [FAIL] $($kv.Key) is ${kb} KB (limit: ${limKb} KB)" -ForegroundColor Red
            $feViolations++
        } else {
            $bundleSummary += "$(Split-Path $kv.Key -Leaf)=${kb}KB/${limKb}KB"
        }
    }
}
$violations += $feViolations
if ($feViolations -eq 0) {
    Write-Host "  [OK] Zero forbidden npm deps; bundles within budget ($($bundleSummary -join ', '))." -ForegroundColor Green
}

Write-Host "=== [5/$totalSteps] Checking Rust <-> TypeScript WebView IPC contract parity ==="
$ipcViolations = 0
$bridgePath = "crates\tomoyo-webview-host\src\bridge.rs"
$bridgeRaw = Get-Content $bridgePath -Raw
$enumMatch = [regex]::Match($bridgeRaw, 'pub enum HostMessage\s*\{([\s\S]*?)\r?\n\}')
if (-not $enumMatch.Success) {
    Write-Host "  [FAIL] Could not locate `pub enum HostMessage` in $bridgePath" -ForegroundColor Red
    $ipcViolations++
} else {
    $validCmds = @{}
    foreach ($line in ($enumMatch.Groups[1].Value -split "\r?\n")) {
        $m = [regex]::Match($line, '^\s+([A-Z][A-Za-z0-9]*)\b')
        if ($m.Success) {
            $pascal = $m.Groups[1].Value
            $snake = [regex]::Replace($pascal, '([a-z0-9])([A-Z])', '$1_$2').ToLowerInvariant()
            $validCmds[$snake] = $true
        }
    }
    $feFiles = Get-ChildItem -Path frontend\src -Recurse -Include *.ts,*.svelte
    $checkedCmds = @{}
    foreach ($ff in $feFiles) {
        $src = Get-Content $ff.FullName -Raw
        $matches = [regex]::Matches($src, 'postHost\s*\(\s*\{\s*cmd\s*:\s*[''"]([a-z0-9_]+)[''"]')
        foreach ($cm in $matches) {
            $cmd = $cm.Groups[1].Value
            $checkedCmds[$cmd] = $true
            if (-not $validCmds.ContainsKey($cmd)) {
                $rel = $ff.FullName.Replace((Get-Location).Path + "\", "")
                Write-Host "  [FAIL] Frontend sends unknown IPC cmd '$cmd' in $rel (missing from HostMessage)" -ForegroundColor Red
                $ipcViolations++
            }
        }
    }
    if ($ipcViolations -eq 0) {
        Write-Host "  [OK] Frontend -> Rust: all $($checkedCmds.Count) IPC commands match Rust HostMessage ($($validCmds.Count) variants)." -ForegroundColor Green
    }

    # Symmetric Rust -> Frontend HostEvent parity check
    $rustEvents = @{}
    foreach ($rf in (Get-ChildItem -Path crates\tomoyo-webview-host\src -Recurse -Include *.rs)) {
        $rsrc = Get-Content $rf.FullName -Raw
        foreach ($em in [regex]::Matches($rsrc, '"event"\s*:\s*"([a-z0-9_]+)"')) {
            $rustEvents[$em.Groups[1].Value] = $true
        }
    }
    $feEvents = @{}
    foreach ($ff in $feFiles) {
        $src = Get-Content $ff.FullName -Raw
        foreach ($em in [regex]::Matches($src, '(?:\bev\b|data\.event)\s*[!=]==\s*[''"]([a-z0-9_]+)[''"]')) {
            $evName = $em.Groups[1].Value
            $feEvents[$evName] = $true
            if (-not $rustEvents.ContainsKey($evName)) {
                $rel = $ff.FullName.Replace((Get-Location).Path + "\", "")
                Write-Host "  [FAIL] Frontend listens for unknown Rust HostEvent '$evName' in $rel" -ForegroundColor Red
                $ipcViolations++
            }
        }
    }
    foreach ($reName in $rustEvents.Keys) {
        if (-not $feEvents.ContainsKey($reName)) {
            Write-Host "  [FAIL] Rust emits unhandled HostEvent '$reName' (no frontend listener found)" -ForegroundColor Red
            $ipcViolations++
        }
    }
    if ($ipcViolations -eq 0) {
        Write-Host "  [OK] Rust -> Frontend: symmetric 1:1 HostEvent parity ($($feEvents.Count)/$($rustEvents.Count) events matched)." -ForegroundColor Green
    }
}
$violations += $ipcViolations

Write-Host "=== [6/$totalSteps] Executing WebView2 UI JS runtime smoke gate ==="
$nodeCmd = Get-Command node -ErrorAction SilentlyContinue
if ($nodeCmd) {
    & $nodeCmd.Source "scripts/check_webview_ui.mjs"
    if ($LASTEXITCODE -ne 0) {
        Write-Host "  [FAIL] WebView2 UI runtime JS smoke check failed!" -ForegroundColor Red
        $violations++
    }
} elseif ($env:CI) {
    Write-Host "  [FAIL] Node.js required in CI to execute scripts/check_webview_ui.mjs" -ForegroundColor Red
    $violations++
} else {
    Write-Host "  [WARN] Node.js not found locally; skipped WebView2 UI JS runtime check." -ForegroundColor Yellow
}

if ($CheckBinaries) {
    Write-Host "=== [7/$totalSteps] Checking release binary size ceilings ==="
    $binBudgets = [ordered]@{
        "target\release\tomoyo.exe"              = 6.5
        "target\release\tomoyo-webview-host.exe" = 5.5
        "target\release\tomoyo-capture.exe"      = 1.5
        "target\release\tomoyo-plugin-ocr.exe"       = 0.8
        "target\release\tomoyo-plugin-notes.exe"     = 0.8
        "target\release\tomoyo-plugin-md-editor.exe" = 0.8
    }
    $binViolations = 0
    $binSummary = @()
    foreach ($kv in $binBudgets.GetEnumerator()) {
        if (-not (Test-Path $kv.Key)) {
            Write-Host "  [FAIL] Missing release binary: $($kv.Key)" -ForegroundColor Red
            $binViolations++
            continue
        }
        $mb = [math]::Round((Get-Item $kv.Key).Length / 1MB, 2)
        if ($mb -gt $kv.Value) {
            Write-Host "  [FAIL] $($kv.Key) is ${mb} MB (limit: $($kv.Value) MB)" -ForegroundColor Red
            $binViolations++
        } else {
            $binSummary += "$(Split-Path $kv.Key -Leaf)=${mb}MB/$($kv.Value)MB"
        }
    }
    $violations += $binViolations
    if ($binViolations -eq 0) {
        Write-Host "  [OK] All release binaries within size budget ($($binSummary -join ', '))." -ForegroundColor Green
    }
}

if ($violations -gt 0) {
    Write-Host "`n[FAILED] Hygiene gate failed with $violations violation(s). See docs/CODING_STANDARDS.md." -ForegroundColor Red
    exit 1
}

Write-Host "`n[PASS] All Tomoyo coding standards & anti-bloat checks passed!" -ForegroundColor Green
exit 0
