# Research: Universal Agent Watcher & Cross-Platform Media Engine Architecture

**Document ID:** `docs/research/07-universal-agent-watcher-and-media-engine-research.md`  
**Date:** 2026-10-15  
**Status:** Validated Research & Architecture Foundation  
**Authors:** NEXUS Architecture & Core Engineering  
**Cross-References:**  
- `docs/features/122-agent-completion-watcher-and-keep-awake-architecture.md`  
- `docs/features/126-universal-agent-watcher-and-cross-platform-music-control-plan.md`  
- `docs/changes/131-universal-agent-watcher-and-cross-platform-music-control.md`  

---

## 1. Executive Summary & Problem Space

Modern developer workflows are defined by long-running asynchronous processes: external coding agents (Claude Code, Codex CLI, Antigravity, Cursor Composer, Trae, Windsurf, Aider), Model Context Protocol (MCP) tool chains, local compilation tasks, and background test runners. Simultaneously, developer audio environments demand seamless, hands-free media playback control across diverse desktop operating systems (macOS on MacBook Pro and Windows 11 on workstations) toggling between Spotify and Apple Music.

Prior to this work, two distinct friction points existed in NEXUS:

1. **The "3-Tool Trap" in Agent Monitoring**: Initial designs restricted task monitoring to a brittle, hardcoded tri-agent set (Claude, Codex, Antigravity). If a developer executed tasks inside Cursor, Trae, Windsurf, a headless MCP runner, or a generic PowerShell/zsh terminal, NEXUS was blind. Furthermore, if a developer spoke *"Nexus, watch over this and update me when it has finished running"*, the system required explicit tool naming rather than autonomously inspecting the active foreground window. Finally, long-running agent tasks allowed the host operating system's power management to enter sleep or display turn-off, cutting network connections and killing CLI execution midway.
2. **Platform & Ecosystem Fragmentation in Media Control**: Media playback was either missing or platform-siloed. On macOS, controlling Apple Music or Spotify requires distinct AppleEvents/AppleScript dictionaries; on Windows, keyboard hardware simulation (`VK_MEDIA_*`) operates universally across players but cannot initiate specific track/artist searches without URI protocol schemes (`spotify:search:`, `applemusic:`). No unified NLU intent grammar or configuration setting bridged user preference between Apple Music and Spotify across operating systems.

This research document analyzes the OS-level primitives, inter-process communication (IPC) contracts, and timing dynamics required to achieve **universal agent completion detection**, **zero-sleep keep-awake leasing**, and **cross-platform Apple Music/Spotify control**.

---

## 2. Universal Coding Agent Monitoring Architecture

```
                                  +---------------------------------------+
                                  | User Spoken Intent:                   |
                                  | "Nexus watch over this and update me" |
                                  +-------------------+-------------------+
                                                      |
                                                      v
                                  +---------------------------------------+
                                  | Intent Parser: ParsedIntent::Watch... |
                                  +-------------------+-------------------+
                                                      |
                         +----------------------------+----------------------------+
                         | (Target specified)                                      | (Implicit: agent == None)
                         v                                                         v
          +-------------------------------+                         +-------------------------------+
          | Explicit Target Resolution:   |                         | Active Foreground Window      |
          | "claude", "cursor", etc.      |                         | Inspection: GetForegroundWin  |
          +---------------+---------------+                         +---------------+---------------+
                          |                                                         |
                          +----------------------------+----------------------------+
                                                       |
                                                       v
                                    +--------------------------------------+
                                    | Acquire KeepAwakeLease ("agent_watch")|
                                    | Win32: SetThreadExecutionState(...)  |
                                    +------------------+-------------------+
                                                       |
                         +-----------------------------+-----------------------------+
                         |                                                           |
                         v                                                           v
      +--------------------------------------+                   +--------------------------------------+
      | CLI / Terminal Mode (PTY Stream):    |                   | Desktop IDE Mode (Cursor/Trae/Wind): |
      | pty-manager + Adapters Server        |                   | Process Tree & Workspace Burst Watch |
      +------------------+-------------------+                   +------------------+-------------------+
                         |                                                           |
                         | ANSI token stripping                                      | File system change debouncing
                         | Regex boundary matching                                   | Process CPU decay (< 1.5%)
                         v                                                           v
      +--------------------------------------+                   +--------------------------------------+
      | 3.0s Quiet-Window Debounce Gate      |                   | 5.0s File Quiescence Gate            |
      +------------------+-------------------+                   +------------------+-------------------+
                         |                                                           |
                         +-----------------------------+-----------------------------+
                                                       |
                                                       v
                                    +--------------------------------------+
                                    | Release KeepAwakeLease               |
                                    +------------------+-------------------+
                                                       |
                                                       v
                                    +--------------------------------------+
                                    | Proactive Spoken Alert:              |
                                    | "Your Claude response is ready, sir."|
                                    +--------------------------------------+
```

### 2.1 The Failure of Hardcoded Tools (The "3-Tool Trap")

Early prototypes bound completion detection directly to specific CLI process names: `claude.exe`, `codex.exe`, and `antigravity.exe`. In production engineering, this breaks immediately due to:
- **Wrapper Executables**: Tools like Claude Code are launched via `npx @anthropic-ai/claude-code`, `bunx`, or global shell scripts (`claude.cmd`, `node.exe`).
- **Modern IDEs**: Cursor, Trae, Windsurf, and VS Code run internal AI agents within Electron rendering processes where no standalone CLI process represents the active turn.
- **Headless MCP Servers**: Tool tasks run inside MCP server sub-processes communicating over JSON-RPC stdin/stdout without any user-facing terminal.

**Requirement**: The watcher must decouple *adapter parsing* from the *target identity*, supporting an extensible adapter registry while providing a generic fallback based on process tree monitoring and workspace file burst quiescence.

### 2.2 Dual Detection Paradigms

To support both command-line agents and GUI IDEs, NEXUS implements two complementary detection paradigms:

#### Paradigm A: PTY Pseudo-Terminal Stream Inspection (CLI Agents)
For tools executing in a terminal, monitoring raw standard streams (stdout/stderr) yields ANSI escape codes, terminal cursor jumps (`\x1b[2K`), and progress bars.
- **Architecture**: A lightweight Node.js sidecar (`pty-manager`) spawns or attaches to the process pseudo-terminal.
- **ANSI Token Stripping**: Raw output is stripped of ANSI control sequences using `strip-ansi`.
- **Adapter Token Contracts**:
  - `ClaudeAdapter`: Matches completion markers (`"Task completed"`, `"Cost:"`, `"Total duration:"`) and prompt markers (`"╭── Claude"`, `"\n> "`).
  - `CodexAdapter`: Matches `"[Codex] Finished execution"` and return prompts.
  - `AntigravityAdapter`: Matches `"[AGY] Run finished"`, `"Done in "`, and `"\x1b[32m✔\x1b[39m Build succeeded"`.
  - `GenericTerminalAdapter`: Detects prompt re-entry patterns (`PS C:\...>`, `user@host:~$ `, `(\w+) [\$#] `) following sustained output.
- **Quiet-Window Debounce**: LLMs emit tokens with variable latency. If a tool pauses for 800ms while generating code, a naive prompt-detector might fire prematurely. A strict **3.0-second quiet window** is enforced: completion triggers only after the termination pattern matches AND no further stdout bytes arrive for 3000ms.

#### Paradigm B: Desktop IDE & Workspace Burst Quiescence (Cursor, Trae, Windsurf, VS Code)
GUI-based IDEs do not expose PTY streams to external processes. However, AI agents running inside IDEs exhibit distinct physical footprints:
1. **CPU Burst Profile**: During inference, planning, and code synthesis, the language server / Electron helper process consumes 15%–80% CPU. When finished and awaiting user input, CPU decays to < 1.5%.
2. **File System Burst Write Quiescence**: During code generation, agents write batches of files in rapid succession. By monitoring the active workspace directory with a non-recursive change notifier, NEXUS observes the write burst. When 5.0 seconds elapse with zero file writes and process CPU remains below baseline, the turn is declared complete.

### 2.3 Implicit Foreground Target Resolution

When a developer commands *"Nexus, watch over this"*, requiring them to specify the tool name introduces cognitive overhead. NEXUS resolves this via Win32 active window introspection:

```rust
#[cfg(target_os = "windows")]
pub fn inspect_active_foreground_target() -> Option<AgentTargetInfo> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 { return None; }
        
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
        let title = String::from_utf16_lossy(&title_buf[..len as usize]);
        
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        
        // Resolve process name and map to known adapter or generic IDE
        resolve_agent_from_window_context(&title, pid)
    }
}
```

The title and process name are mapped through `KNOWN_ADAPTERS`:
- `"Cursor"` / `"cursor.exe"` → Target: Cursor
- `"Trae"` / `"trae.exe"` → Target: Trae
- `"Antigravity"` / `"antigravity.exe"` → Target: Antigravity
- `"Visual Studio Code"` / `"Code.exe"` → Target: VS Code
- `"Windows PowerShell"` / `"cmd.exe"` / `"wt.exe"` → Target: Generic Terminal / Active CLI

---

## 3. OS Power Management & Keep-Awake Architecture

### 3.1 The Problem: Premature System Sleep During Long Runs
When an agent is performing large-scale refactoring, running end-to-end test suites, or training local models, user interaction ceases. Operating system power policies step in after 5–15 minutes:
1. Windows suspends execution or enters Connected Standby (`Modern Standby`).
2. macOS puts CPU cores into low-power idle or suspends background threads.
3. Network sockets disconnect, breaking remote LLM streaming connections and aborting long tasks.

### 3.2 OS-Level Keep-Awake Implementation Matrix

| Operating System | API / Mechanism | Flags / Parameters | Behavior |
| :--- | :--- | :--- | :--- |
| **Windows 10/11** | `SetThreadExecutionState` (`kernel32.dll`) | `ES_CONTINUOUS \| ES_SYSTEM_REQUIRED \| ES_AWAYMODE_REQUIRED` | Prevents sleep and idle timeouts; system stays awake even if display sleeps. |
| **macOS** | `IOPMAssertionCreateWithName` (`IOKit.framework`) | `kIOPMAssertionTypePreventUserIdleSystemSleep` | Holds a power assertion preventing Darwin kernel sleep while task runs. |
| **Linux** | `systemd-inhibit` or D-Bus ScreenSaver | `--what=idle:sleep --why="Agent task running"` | Requests desktop inhibitor lock over D-Bus (`org.freedesktop.ScreenSaver`). |

### 3.3 The RAII Lease Pattern (`KeepAwakeLease`)

To prevent "stuck awake" bugs (where the system never sleeps because a task died or threw an unhandled error), keep-awake is wrapped in an atomic RAII guard:

```rust
pub struct KeepAwakeLease {
    reason: String,
    active: Arc<AtomicBool>,
}

impl KeepAwakeLease {
    pub fn acquire(reason: &str) -> Self {
        set_os_keep_awake(true);
        Self {
            reason: reason.to_string(),
            active: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl Drop for KeepAwakeLease {
    fn drop(&mut self) {
        if self.active.swap(false, Ordering::SeqCst) {
            set_os_keep_awake(false);
        }
    }
}
```

When the watcher loop terminates—whether by successful completion, user abort via hotkey, timeout, or panic—the `KeepAwakeLease` drops, restoring native OS power management immediately.

---

## 4. Cross-Platform Media Engine Architecture

```
                          +---------------------------------------+
                          | User Spoken Intent:                   |
                          | "Play Bohemian Rhapsody on Spotify"   |
                          | "Open Apple Music"                    |
                          | "Next track" / "Pause songs"          |
                          +-------------------+-------------------+
                                              |
                                              v
                          +---------------------------------------+
                          | Intent Parser:                        |
                          | - ParsedIntent::PlayMusic             |
                          | - ParsedIntent::OpenMusicPlayer       |
                          +-------------------+-------------------+
                                              |
                                              v
                          +---------------------------------------+
                          | Orchestrator / Command Executor:      |
                          | Fetch user preference:                |
                          | settings.default_music_player         |
                          +-------------------+-------------------+
                                              |
                     +------------------------+------------------------+
                     |                                                 |
             [macOS Target]                                    [Windows Target]
                     v                                                 v
  +-------------------------------------+           +-------------------------------------+
  | AppleScript via osascript:          |           | Win32 Virtual Key & URI Engine:     |
  | Target: "Music" or "Spotify"        |           | Target: Spotify / Apple Music       |
  +------------------+------------------+           +------------------+------------------+
                     |                                                 |
     +---------------+---------------+                 +---------------+---------------+
     |                               |                 |                               |
     v                               v                 v                               v
[Transport]                    [Search / Play]   [Transport]                    [Search / Deep Link]
`tell application "Music"       `open -a "Music"` `keybd_event(0xB3)`            `start spotify:search:...`
 to playpause`                  or spotify URI    (VK_MEDIA_PLAY_PAUSE)         or `start applemusic:...`
```

### 4.1 macOS Architecture: Mach Ports & AppleEvents via `osascript`

macOS provides deep programmatic access to both Apple Music (`Music.app`) and Spotify (`Spotify.app`) via Open Scripting Architecture (OSA) AppleEvents:

#### Apple Music (`Music.app`) Control Contracts:
```applescript
-- Play/Pause toggle
tell application "Music" to playpause

-- Next / Previous Track
tell application "Music" to next track
tell application "Music" to previous track

-- Track Playback / Search
tell application "Music"
    set matchedTracks to (every track whose name contains "QUERY")
    if (count of matchedTracks) > 0 then
        play (first item of matchedTracks)
    end if
end tell
```

#### Spotify (`Spotify.app`) Control Contracts:
```applescript
-- Play/Pause toggle
tell application "Spotify" to playpause

-- Next / Previous Track
tell application "Spotify" to next track
tell application "Spotify" to previous track

-- Play specific Spotify URI
tell application "Spotify" to play track "spotify:track:..."
```

Execution is performed natively via `std::process::Command::new("osascript").arg("-e").arg(script)`.

### 4.2 Windows Architecture: Win32 Virtual Key Simulation & Deep Linking

Unlike macOS, Windows lacks a standardized IPC scripting layer for third-party desktop apps. However, it provides:
1. **Universal Hardware Media Keys**: Windows handles multimedia keys at the kernel/DWM level. Any active media session (Spotify desktop, Apple Music preview, Chrome YouTube, edge audio) honors virtual key synthesis via `keybd_event` or `SendInput`.
2. **Protocol URI Handlers**: Both Spotify and Apple Music register Windows shell URI schemes for deep navigation and track launching.

#### Win32 Virtual Key Mapping (`keybd_event`):
```rust
pub const VK_MEDIA_NEXT_TRACK: u8 = 0xB0;
pub const VK_MEDIA_PREV_TRACK: u8 = 0xB1;
pub const VK_MEDIA_STOP: u8       = 0xB2;
pub const VK_MEDIA_PLAY_PAUSE: u8  = 0xB3;
pub const VK_VOLUME_MUTE: u8      = 0xAD;
pub const VK_VOLUME_DOWN: u8      = 0xAE;
pub const VK_VOLUME_UP: u8        = 0xAF;

pub fn send_media_key(vk: u8) {
    unsafe {
        // Press
        keybd_event(vk, 0, 0, 0);
        // Release
        keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
    }
}
```

#### Protocol Handler Deep Links:
When the user requests playback of a specific artist or song (e.g. *"Play Starboy on Spotify"*), a hardware key cannot specify the query. NEXUS constructs and launches the official protocol URL:
- **Spotify**: `spotify:search:{url_encoded_query}` or `spotify:track:{id}`
  Executed via: `cmd /c start spotify:search:Starboy`
- **Apple Music**: `applemusic://` or `itms://` or `music://`
  Executed via: `cmd /c start applemusic:`

### 4.3 Linux Architecture: MPRIS D-Bus Specifications

On Linux desktops (GNOME, KDE), media players adhere to the **Media Player Remote Interfacing Specification (MPRIS)** over D-Bus:
- Interface: `org.mpris.MediaPlayer2.Player`
- Targets: `org.mpris.MediaPlayer2.spotify`, `org.mpris.MediaPlayer2.rhythmbox`
- Methods: `PlayPause`, `Next`, `Previous`, `Stop`, `OpenUri`

---

## 5. Comparative Analysis Matrix

| Feature Dimension | macOS (Apple Music) | macOS (Spotify) | Windows (Spotify) | Windows (Apple Music) |
| :--- | :--- | :--- | :--- | :--- |
| **IPC Mechanism** | AppleScript / AppleEvents | AppleScript / AppleEvents | Win32 `VK_MEDIA_*` + URI | Win32 `VK_MEDIA_*` + URI |
| **Transport Latency** | ~35ms | ~40ms | **< 1ms** (kernel key event) | **< 1ms** (kernel key event) |
| **Search & Direct Play** | Native library search | Spotify URI scheme | `spotify:search:...` URI | Windows App URI / Store Link |
| **Track Meta Extraction** | Full (Artist, Album, Art) | Full (Artist, Album, Art) | Limited (via SMTC hook) | Limited (via SMTC hook) |
| **Background Control** | Yes (without window focus) | Yes (without window focus) | Yes (OS session hook) | Yes (OS session hook) |

---

## 6. Architectural Recommendations & Conclusions

1. **Decouple Target Name from Detection Logic**: Do not hardcode a small list of tools. Use `KNOWN_ADAPTERS` for specialized token patterns, but fallback transparently to active foreground window inspection and file/process quiescence.
2. **Enforce RAII Power Management**: Always wrap keep-awake calls in a lease struct (`KeepAwakeLease`) so process exit, crash, or cancellation cannot leak an active keep-awake state on the user's computer.
3. **Store Unified User Preferences**: Support `"spotify"` and `"apple_music"` in `NexusSettings`, allowing developers to set their preferred player once in Settings and have all natural speech commands ("play songs", "pause music", "next") automatically dispatch to the chosen player on both macOS and Windows.
4. **Proactive Spoken Notification**: Do not wait for user polling. On task completion, leverage `proactive_policy` with `Urgency::High` to speak a contextual acknowledgment: *"Your [Agent] response is ready, sir. It has finished running."*
