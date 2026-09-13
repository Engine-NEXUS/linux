# NEXUS Unified Launcher
# Runs the NEXUS desktop app in ONE terminal with color-coded logs.
# All output (Rust wake-word, audio, frontend console, commands)
# appears in a single scrolling view.
#
# Usage:
#   pwsh ./scripts/run.ps1              # normal start
#   pwsh ./scripts/run.ps1 -Build       # rebuild before starting
#   pwsh ./scripts/run.ps1 -Debug       # enable CDP debugging port 9222
#   pwsh ./scripts/run.ps1 -Admin       # start Qwen brain server (admin mode)
#
# Press Ctrl+C to stop everything cleanly.

param(
  [switch]$Build,
  [switch]$Debug,
  [switch]$Admin
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$LogDir = "$env:APPDATA\com.nexus.assistant"
if (-not (Test-Path $LogDir)) { New-Item -ItemType Directory -Path $LogDir -Force | Out-Null }

# ─── Colors ────────────────────────────────────────────────────────────────
$C_STT   = "Cyan"      # STT transcription logs (faster-whisper sidecar)
$C_RUST  = "Green"     # Rust wake-word / audio logs
$C_FRONT = "Yellow"    # Frontend console logs (via CDP)
$C_CMD   = "Magenta"   # Command execution
$C_SYS   = "DarkGray"  # System / launcher messages
$C_ERR   = "Red"       # Errors

function Write-Log([string]$Tag, [string]$Msg, [string]$Color = "White") {
  $ts = Get-Date -Format "HH:mm:ss"
  Write-Host "[$ts] " -NoNewline -ForegroundColor $C_SYS
  Write-Host "$Tag " -NoNewline -ForegroundColor $Color
  Write-Host $Msg
}

# ─── Cleanup helper ────────────────────────────────────────────────────────
$jobs = [System.Collections.ArrayList]::new()
$cts = [System.Threading.CancellationTokenSource]::new()

function Stop-All {
  Write-Log "STOP" "Shutting down all processes..." $C_ERR
  foreach ($j in $jobs) {
    try {
      if ($j.Process -and -not $j.Process.HasExited) {
        $j.Process.Kill()
      }
    } catch {}
  }
  Get-Process nexus -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Get-Process node -ErrorAction SilentlyContinue | Where-Object { $_.CommandLine -like "*cdp_monitor*" } | Stop-Process -Force -ErrorAction SilentlyContinue
  Write-Log "STOP" "All processes stopped." $C_ERR
}

trap {
  Write-Log "ERR" "Unhandled error: $_" $C_ERR
  Stop-All
  exit 1
}

# ─── Build if requested ────────────────────────────────────────────────────
if ($Build) {
  Write-Log "BUILD" "Building frontend..." $C_SYS
  npm --prefix frontend run build 2>&1 | Out-Host
  Write-Log "BUILD" "Building Rust (release + custom-protocol)..." $C_SYS
  Push-Location src-tauri
  cargo build --release --features custom-protocol 2>&1 | Out-Host
  Pop-Location
  if ($LASTEXITCODE -ne 0) {
    Write-Log "BUILD" "Build FAILED" $C_ERR
    exit 1
  }
  Write-Log "BUILD" "Build complete." $C_SYS
}

# ─── Kill any existing instances ───────────────────────────────────────────
Write-Log "INIT" "Killing existing NEXUS instances..." $C_SYS

# Build a set of nexus.exe PIDs so we can kill only OUR WebView2 children.
# Killing ALL msedgewebview2.exe processes would also kill WhatsApp, M365
# Copilot, Windows Search, and any other app that embeds WebView2.
$nexusPids = (Get-Process nexus -ErrorAction SilentlyContinue).Id
if ($nexusPids) {
  $nexusPidSet = [System.Collections.Generic.HashSet[int]]::new()
  foreach ($p in $nexusPids) { [void]$nexusPidSet.Add($p) }

  # Walk the process tree to find all descendant PIDs of nexus.exe
  $allProcs = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
  $parentMap = @{}
  foreach ($proc in $allProcs) { $parentMap[$proc.ProcessId] = $proc.ParentProcessId }
  $queue = [System.Collections.Generic.Queue[int]]::new()
  foreach ($p in $nexusPids) { $queue.Enqueue($p) }
  $descendants = [System.Collections.Generic.HashSet[int]]::new()
  while ($queue.Count -gt 0) {
    $cur = $queue.Dequeue()
    foreach ($kv in $parentMap.GetEnumerator()) {
      if ($kv.Value -eq $cur -and $descendants.Add($kv.Key)) {
        $queue.Enqueue($kv.Key)
      }
    }
  }

  # Kill nexus.exe first so it stops spawning new WebView2 children
  Get-Process nexus -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

  # Kill only msedgewebview2.exe processes that are descendants of nexus.exe
  Get-Process msedgewebview2 -ErrorAction SilentlyContinue | Where-Object {
    $descendants.Contains($_.Id)
  } | Stop-Process -Force -ErrorAction SilentlyContinue
} else {
  # No nexus running — nothing to clean up
  Write-Log "INIT" "No existing NEXUS process found" $C_SYS
}

Start-Sleep 3

# Clear old log files so the tail loop doesn't read stale content
Write-Log "INIT" "Clearing old logs..." $C_SYS
$nexusLog = "$LogDir\nexus_unified.log"
$nexusErr = "$LogDir\nexus_unified_err.log"
$cdpLog = "$LogDir\cdp_unified.log"
$cdpErr = "$LogDir\cdp_unified_err.log"
foreach ($f in @($nexusLog, $nexusErr, $cdpLog, $cdpErr)) {
  if (Test-Path $f) { Clear-Content $f -Force -ErrorAction SilentlyContinue }
}

# ─── Start NEXUS ───────────────────────────────────────────────────────────
# STT is lazy-started by Rust (faster-whisper on port 39217) — no external server needed at boot.
Write-Log "INIT" "Starting NEXUS desktop app..." $C_RUST

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = if ($Debug) { "--remote-debugging-port=9222" } else { "" }

$nexusProc = Start-Process -FilePath "$ProjectRoot\src-tauri\target\release\nexus.exe" `
  -RedirectStandardOutput $nexusLog `
  -RedirectStandardError $nexusErr `
  -PassThru -WindowStyle Hidden

$jobs.Add([PSCustomObject]@{ Name="NEXUS"; Process=$nexusProc }) | Out-Null

# Wait for NEXUS to start
Start-Sleep 5
if ($nexusProc.HasExited) {
  Write-Log "NEXUS" "CRASHED on startup — check $nexusErr" $C_ERR
  Get-Content $nexusErr | ForEach-Object { Write-Log "NEXUS" $_ $C_ERR }
  Stop-All
  exit 1
}
Write-Log "NEXUS" "App running (PID=$($nexusProc.Id))" $C_RUST

# ─── Start Qwen Brain Server (admin mode) ──────────────────────────────────
if ($Admin) {
  $brainScript = "$ProjectRoot\server\admin\brain_server.py"
  $adminConfig = "$ProjectRoot\server\admin\admin_config.json"
  $brainModel = "$ProjectRoot\server\admin\model\qwen2.5-0.5b-instruct-q4_k_m.gguf"

  # Kill any existing brain server on port 39219 (from a previous session)
  $oldBrain = Get-NetTCPConnection -LocalPort 39219 -ErrorAction SilentlyContinue
  if ($oldBrain) {
    $oldPid = $oldBrain.OwningProcess | Select-Object -Unique
    foreach ($p in $oldPid) {
      $proc = Get-Process -Id $p -ErrorAction SilentlyContinue
      if ($proc) {
        Write-Log "BRAIN" "Killing old brain server (PID=$p)..." "DarkMagenta"
        Stop-Process -Id $p -Force -ErrorAction SilentlyContinue
        Start-Sleep 1
      }
    }
  }

  if (-not (Test-Path $brainScript)) {
    Write-Log "BRAIN" "brain_server.py not found at $brainScript" $C_ERR
    Write-Log "BRAIN" "Admin mode requires server/admin/brain_server.py" $C_ERR
  } elseif (-not (Test-Path $adminConfig)) {
    Write-Log "BRAIN" "admin_config.json not found — creating default..." $C_SYS
    $defaultConfig = @{
      is_admin = $true
      brain_enabled = $true
      brain_port = 39219
      brain_model = "qwen2.5-0.5b-instruct-q4_k_m.gguf"
      auto_train = $true
      retrain_threshold = 50
      min_confidence = 0.90
    } | ConvertTo-Json -Depth 3
    Set-Content -Path $adminConfig -Value $defaultConfig -Encoding UTF8
    Write-Log "BRAIN" "Created admin_config.json with defaults" $C_SYS
  }

  if (Test-Path $brainScript) {
    # Check if brain model exists
    if (-not (Test-Path $brainModel)) {
      Write-Log "BRAIN" "Qwen model not found at $brainModel" $C_ERR
      Write-Log "BRAIN" "Download qwen2.5-0.5b-instruct-q4_k_m.gguf (~398MB)" $C_ERR
      Write-Log "BRAIN" "Place it in server/admin/model/" $C_ERR
    } else {
      $brainLog = "$LogDir\brain_server.log"
      $brainErr = "$LogDir\brain_server_err.log"
      if (Test-Path $brainLog) { Clear-Content $brainLog -Force -ErrorAction SilentlyContinue }
      if (Test-Path $brainErr) { Clear-Content $brainErr -Force -ErrorAction SilentlyContinue }

      Write-Log "BRAIN" "Starting Qwen brain server (port 39219)..." "Magenta"
      $brainProc = Start-Process -FilePath "python" `
        -ArgumentList $brainScript `
        -WorkingDirectory "$ProjectRoot\server\admin" `
        -RedirectStandardOutput $brainLog `
        -RedirectStandardError $brainErr `
        -PassThru -WindowStyle Hidden
      $jobs.Add([PSCustomObject]@{ Name="BRAIN"; Process=$brainProc }) | Out-Null

      # Wait a moment and check if it crashed
      Start-Sleep 3
      if ($brainProc.HasExited) {
        Write-Log "BRAIN" "Brain server CRASHED — check $brainErr" $C_ERR
        Get-Content $brainErr | ForEach-Object { Write-Log "BRAIN" $_ $C_ERR }
      } else {
        Write-Log "BRAIN" "Brain server starting (PID=$($brainProc.Id)) — model loads in ~10s" "Magenta"
        Write-Log "BRAIN" "  Port: 39219  Model: Qwen2.5-0.5B-Instruct (398MB GGUF)" "DarkMagenta"
        Write-Log "BRAIN" "  Continuous learning: ON  Auto-train: ON" "DarkMagenta"
      }
    }
  }
}

# ─── Start CDP monitor (frontend console logs) ────────────────────────────
$cdpScript = "$ProjectRoot\scripts\cdp_monitor.js"
if ($Debug -and (Test-Path $cdpScript)) {
  Write-Log "INIT" "Starting CDP console monitor..." $C_FRONT
  $cdpProc = Start-Process -FilePath "node" `
    -ArgumentList $cdpScript `
    -WorkingDirectory $ProjectRoot `
    -RedirectStandardOutput "$LogDir\cdp_unified.log" `
    -RedirectStandardError "$LogDir\cdp_unified_err.log" `
    -PassThru -WindowStyle Hidden
  $jobs.Add([PSCustomObject]@{ Name="CDP"; Process=$cdpProc }) | Out-Null
}

# ─── Tail all logs in one stream ───────────────────────────────────────────
Write-Log "READY" "═══════════════════════════════════════════════════════" $C_SYS
Write-Log "READY" "  NEXUS Unified Console — all logs below" $C_SYS
Write-Log "READY" "  Rust=Green  Frontend=Yellow  Cmd=Magenta  STT=Cyan" $C_SYS
if ($Admin) {
  Write-Log "READY" "  Brain=Magenta  (Qwen 0.5B on port 39219 — admin mode)" "Magenta"
}
Write-Log "READY" "  Press Ctrl+C to stop everything" $C_SYS
Write-Log "READY" "═══════════════════════════════════════════════════════" $C_SYS
Write-Host ""

# Track file positions for incremental tailing.
# IMPORTANT: These MUST be simple variables, NOT hashtable properties.
# In PowerShell, [ref]$hashtable.Property creates a reference to a boxed
# COPY of the value, not the actual hashtable slot. So updates inside the
# function would be lost and the position would stay at 0 forever, causing
# every cycle to re-read the entire log file from the beginning (the
# "repeating logs" bug).
$posRust = 0
$posCDP = 0
$posErr = 0
$posBrain = 0

function Get-NewLines([string]$File, [ref]$Position) {
  if (-not (Test-Path $File)) { return @() }
  $fi = Get-Item $File
  if ($fi.Length -lt $Position.Value) {
    # File was truncated/rotated — start from beginning
    $Position.Value = 0
  }
  if ($fi.Length -eq $Position.Value) { return @() }
  $lines = @()
  try {
    $fs = [System.IO.File]::Open($File, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
    $fs.Seek($Position.Value, [System.IO.SeekOrigin]::Begin) | Out-Null
    # Read raw bytes instead of using StreamReader — StreamReader buffers
    # ahead, so $fs.Position ends up PAST the actual data, causing the
    # position tracker to reset to 0 on the next call and re-read everything
    $len = [int]($fs.Length - $Position.Value)
    $buf = New-Object byte[] $len
    $read = $fs.Read($buf, 0, $len)
    $Position.Value = $Position.Value + $read
    $fs.Close()
    $text = [System.Text.Encoding]::UTF8.GetString($buf, 0, $read)
    $lines = $text -split "`r?`n" | Where-Object { $_ -ne "" }
  } catch {}
  return $lines
}

# Main tail loop
try {
  while (-not $nexusProc.HasExited) {
    Start-Sleep -Milliseconds 500

    # Rust logs (wake word, audio, baton pass) — limit to 50 lines per cycle
    $rustLines = Get-NewLines $nexusLog ([ref]$posRust)
    $rustShown = 0
    foreach ($line in $rustLines) {
      if ($rustShown -ge 50) { break }
      # Strip ANSI color codes
      $clean = $line -replace '\x1b\[[0-9;]*m', ""
      # Extract timestamp and level
      if ($clean -match "(\d{2}:\d{2}:\d{2}\.\d+).*?(INFO|DEBUG|WARN|ERROR|TRACE)\s+(.+)") {
        $level = $Matches[2]
        $msg = $Matches[3]
        # Skip TRACE entirely (AGC gain etc — too noisy)
        if ($level -eq "TRACE") { continue }
        $color = switch ($level) {
          "INFO"  { $C_RUST }
          "DEBUG" { "DarkGreen" }
          "WARN"  { "Yellow" }
          "ERROR" { $C_ERR }
          default { "White" }
        }
        # Highlight key events
        if ($msg -match "NEXUS detected|wake.*trigger") {
          Write-Log "WAKE" $msg $C_CMD; $rustShown++
        } elseif ($msg -match "stream paused|stream resumed|baton") {
          Write-Log "BATON" $msg $C_CMD; $rustShown++
        } elseif ($msg -match "model probability") {
          # Only show probabilities above 0.3
          if ($msg -match "probability=0\.[3-9]|probability=1\.") {
            Write-Log "WAKE" $msg "DarkYellow"; $rustShown++
          }
        } elseif ($msg -match "stream started|device|sample_rate|audio capture started") {
          Write-Log "AUDIO" $msg $C_RUST; $rustShown++
        } elseif ($msg -match "callbacks.*processed") {
          # Only show every 5000 callbacks (not every 1000)
          if ($msg -match "(\d+) callbacks" -and [int64]$Matches[1] % 5000 -eq 0) {
            Write-Log "AUDIO" $msg "DarkGray"; $rustShown++
          }
        } elseif ($level -eq "INFO") {
          Write-Log "RUST" $msg $color; $rustShown++
        } elseif ($level -eq "WARN" -or $level -eq "ERROR") {
          Write-Log "RUST" $msg $color; $rustShown++
        }
        # Skip all other DEBUG lines (audio passed gate, AGC, etc)
      }
    }

    # Frontend CDP logs
    if ($Debug -and (Test-Path "$LogDir\cdp_unified.log")) {
      $cdpLines = Get-NewLines "$LogDir\cdp_unified.log" ([ref]$posCDP)
      foreach ($line in $cdpLines) {
        $clean = $line -replace '\x1b\[[0-9;]*m', ""
        if ($clean -match "\[log\]\s*(.+)") {
          $msg = $Matches[1]
          if ($msg -match "baton pass|pause_wakeword|resume_wakeword") {
            Write-Log "BATON" $msg $C_CMD
          } elseif ($msg -match "VAD.*speech|VAD.*silence|VAD.*misfire") {
            # Only show speech start/end, not every frame
            if ($msg -match "speech start|speech end|speech real|misfire") {
              Write-Log "VAD" $msg $C_FRONT
            }
          } elseif ($msg -match "STT correction|transcript=|intent=|isLongRunning") {
            Write-Log "STT" $msg $C_STT
          } elseif ($msg -match "result:|sendTranscript|sidebar:|ackLong") {
            Write-Log "CMD" $msg $C_CMD
          } elseif ($msg -match "TTS|speak|WebSpeech") {
            Write-Log "TTS" $msg "DarkYellow"
          } elseif ($msg -match "wake|__NEXUS") {
            Write-Log "WAKE" $msg $C_FRONT
          } elseif ($msg -match "didn't catch|retry") {
            Write-Log "RETRY" $msg $C_CMD
          } else {
            Write-Log "UI" $msg $C_FRONT
          }
        }
      }
    }

    # NEXUS stderr (errors)
    $errLines = Get-NewLines $nexusErr ([ref]$posErr)
    foreach ($line in $errLines) {
      $clean = $line -replace '\x1b\[[0-9;]*m', ""
      if ($clean -match "sending transcript|worker response") {
        Write-Log "NET" $clean $C_CMD
      } elseif ($clean.Length -gt 5 -and $clean -notmatch "registry key|Chrome_WidgetWin") {
        Write-Log "ERR" $clean $C_ERR
      }
    }

    # Brain server logs (admin mode)
    if ($Admin -and (Test-Path "$LogDir\brain_server.log")) {
      $brainLines = Get-NewLines "$LogDir\brain_server.log" ([ref]$posBrain)
      foreach ($line in $brainLines) {
        $clean = $line -replace '\x1b\[[0-9;]*m', ""
        if ($clean.Length -gt 5) {
          if ($clean -match "classify|phrasing|pronunciation|train") {
            Write-Log "BRAIN" $clean "Magenta"
          } elseif ($clean -match "ERROR|error|Traceback") {
            Write-Log "BRAIN" $clean $C_ERR
          } elseif ($clean -match "startup|ready|loaded|model") {
            Write-Log "BRAIN" $clean "DarkMagenta"
          }
        }
      }
    }
  }
} finally {
  Write-Log "EXIT" "NEXUS process exited." $C_ERR
  Stop-All
}
