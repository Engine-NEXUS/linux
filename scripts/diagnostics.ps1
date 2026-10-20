# NEXUS Connection Diagnostics
# Checks: STT, TTS, Cloudflare Worker, GitHub, Google
# STT is faster-whisper tiny.en (lazy-started Python sidecar on port 39217).

$cfg = "$env:APPDATA\com.nexus.assistant\nexus-config.json"
$worker = ""
$uid = ""

if (Test-Path $cfg) {
    $j = Get-Content $cfg -Raw | ConvertFrom-Json
    $worker = $j.serverUrl
    $uid = $j.userId
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  NEXUS Connection Diagnostics" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# 1. STT (faster-whisper tiny.en — lazy-started on port 39217)
Write-Host -NoNewline "  [STT]    "
$sttHealth = try { (Invoke-WebRequest -Uri "http://127.0.0.1:39217/health" -TimeoutSec 3 -UseBasicParsing).StatusCode } catch { 0 }
$nexusProc = Get-Process nexus -ErrorAction SilentlyContinue
if ($sttHealth -eq 200) {
    Write-Host "READY" -ForegroundColor Green
    Write-Host "           faster-whisper tiny.en on port 39217"
} elseif ($nexusProc) {
    Write-Host "LAZY" -ForegroundColor Yellow
    Write-Host "           NEXUS running — STT starts on first wake (port 39217)"
} else {
    Write-Host "NOT RUNNING" -ForegroundColor Yellow
    Write-Host "           Start NEXUS first: nexus start"
}

# 2. TTS
Write-Host -NoNewline "  [TTS]    "
$settingsPath = "$env:APPDATA\com.nexus.assistant\settings.json"
$ttsProviders = @("Web Speech (fallback)")
if (Test-Path $settingsPath) {
    $content = Get-Content $settingsPath -Raw
    if ($content -match "gemini|Gemini") { $ttsProviders += "Gemini TTS" }
    if ($content -match "fish|Fish") { $ttsProviders += "Fish Audio" }
    if ($content -match "eleven|Eleven") { $ttsProviders += "ElevenLabs" }
}
Write-Host "AVAILABLE" -ForegroundColor Green
Write-Host "           Providers: $($ttsProviders -join ', ')"

# 3. Cloudflare Worker
Write-Host -NoNewline "  [Worker] "
if ($worker -and $worker -ne "") {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $r = Invoke-WebRequest -Uri ($worker.TrimEnd('/') + "/health") -TimeoutSec 10 -UseBasicParsing
        $sw.Stop()
        Write-Host "CONNECTED" -ForegroundColor Green
        Write-Host "           URL: $worker ($($sw.ElapsedMilliseconds)ms)"
    } catch {
        try {
            $r = Invoke-WebRequest -Uri $worker -TimeoutSec 10 -UseBasicParsing
            $sw.Stop()
            Write-Host "CONNECTED" -ForegroundColor Green
            Write-Host "           URL: $worker ($($sw.ElapsedMilliseconds)ms, root OK)"
        } catch {
            $sw.Stop()
            Write-Host "OFFLINE" -ForegroundColor Red
            Write-Host "           Cannot reach: $worker"
        }
    }
} else {
    Write-Host "NOT CONFIGURED" -ForegroundColor Yellow
    Write-Host "           Set NEXUS_SERVER_URL in settings"
}

# 4. GitHub
Write-Host -NoNewline "  [GitHub] "
if ($worker -and $uid) {
    try {
        $r = Invoke-WebRequest -Uri ($worker.TrimEnd('/') + "/oauth/status?user_id=" + $uid) -TimeoutSec 10 -UseBasicParsing
        $b = $r.Content
        if ($b -match "github.*true") {
            Write-Host "CONNECTED" -ForegroundColor Green
        } else {
            Write-Host "NOT CONNECTED" -ForegroundColor Yellow
            Write-Host "           Run setup wizard to connect"
        }
    } catch {
        Write-Host "UNKNOWN" -ForegroundColor Yellow
        Write-Host "           Worker unreachable"
    }
} else {
    Write-Host "SKIPPED" -ForegroundColor DarkGray
    Write-Host "           No Worker URL or user ID"
}

# 5. Google
Write-Host -NoNewline "  [Google] "
if ($worker -and $uid) {
    try {
        $r = Invoke-WebRequest -Uri ($worker.TrimEnd('/') + "/oauth/status?user_id=" + $uid) -TimeoutSec 10 -UseBasicParsing
        $b = $r.Content
        if ($b -match "google.*true") {
            Write-Host "CONNECTED" -ForegroundColor Green
        } else {
            Write-Host "NOT CONNECTED" -ForegroundColor Yellow
            Write-Host "           Run setup wizard to connect"
        }
    } catch {
        Write-Host "UNKNOWN" -ForegroundColor Yellow
        Write-Host "           Worker unreachable"
    }
} else {
    Write-Host "SKIPPED" -ForegroundColor DarkGray
    Write-Host "           No Worker URL or user ID"
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
