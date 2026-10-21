# Change 96: Ghost Mode Always-On Display Topmost Fix & Mount State Synchronization

## Problem & Motivation

1. **Voice Orb Disappearing Under Windows During Ghost Mode**:
   - The user reported: *"and why is the orb alwasy on diaply not showing in the ghost mode plana nd research what is the cause of this so we can plan accordingly"*.
   - In Ghost Mode, the Voice Orb is designed as an Always-On Display (AOD) head-up display element docked at the top edge of the primary monitor, remaining continuously visible and reactive while the user uses other applications (e.g., Brave Browser, VS Code).
   - In live testing, when the user said *"Open Brave Browser"* during an active Ghost Mode session, the Voice Orb completely vanished from view behind Brave Browser, even though captions and voice actions continued executing in the background.

2. **Root Causes**:
   - **Win32 Z-Order Demotion in `stage.rs`**:
     In Change 90, `stage.rs` had added Win32 logic attempting to place the stage window immediately behind the Windows taskbar (`Shell_TrayWnd`):
     ```rust
     let taskbar = FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR(std::ptr::null()));
     let _ = SetWindowPos(HWND(hwnd.0 as _), taskbar, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
     ```
     Under Windows Win32 API rules, passing any non-topmost HWND (such as `Shell_TrayWnd`) to `SetWindowPos` implicitly strips the `WS_EX_TOPMOST` extended window style. Consequently, the moment any normal desktop application window (like Brave Browser) was activated or focused, Windows placed it above the stage window, occluding the Voice Orb.
   - **Mount-Time Ghost Session State Race Condition**:
     `frontend/src/stage/OrbFrame.tsx` had mount queries for `get_pending_orb_rect` and `get_pending_orb_position`, but did not have a query for active ghost mode sessions. It only listened to live `"ghost:session"` events. If the stage window remounted or was created while a ghost session was already active, `ghostActive` in the frontend store remained `false`. After the first spoken turn completed, `hideOrbAfterSpeech` checked `!s.ghostActive` and hid the Voice Orb.
   - **CDP Monitor Startup Failure in `run.ps1`**:
     `scripts/cdp_monitor.js` failed to resolve the `ws` module when executed from workspace root because `ws` was installed inside `frontend/node_modules`, and it was missing `const http = require('http');`.

---

## Architectural Changes

### 1. Enforced True `HWND_TOPMOST` in `src-tauri/src/stage.rs`
- Replaced the flawed taskbar-relative `SetWindowPos` call with an explicit `HWND_TOPMOST` z-order placement:
  ```rust
  let _ = SetWindowPos(
      HWND(hwnd.0 as _),
      HWND_TOPMOST,
      0, 0, 0, 0,
      SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
  );
  ```
- Because `stage` already enforces `win.set_ignore_cursor_events(true)` across transparent regions (and registers discrete physical hitboxes only on interactive controls via `stage_set_hitboxes`), keeping the stage window at `HWND_TOPMOST` ensures the Voice Orb stays reliably visible above all applications without intercepting clicks on the desktop or other apps.

### 2. Added `get_pending_ghost_session` IPC Query
- **`src-tauri/src/ghost.rs`**:
  - Implemented the command:
    ```rust
    #[tauri::command]
    pub fn get_pending_ghost_session() -> bool {
        session_active()
    }
    ```
- **`src-tauri/src/lib.rs`**:
  - Registered `ghost::get_pending_ghost_session` in the Tauri IPC handler list.

### 3. Mount-Time Ghost State Synchronization in `frontend/src/stage/OrbFrame.tsx`
- Added an initial query on mount:
  ```typescript
  void tauriInvoke("get_pending_ghost_session")
    .then((active) => {
      if (active === true) {
        const s = useAssistant.getState();
        s.setGhostActive(true);
        s.setVisible(true);
        console.log("[ORB] get_pending_ghost_session active=true → visible=true");
      }
    })
    .catch(() => {});
  ```
- Ensures that on initial stage load or any subsequent reloads during an active Ghost Mode session, `ghostActive` and `visible` are immediately synchronized to `true`.

### 4. CDP Monitor Module Resolution & HTTP Import
- **`scripts/cdp_monitor.js`**:
  - Added import `const http = require('http');`.
  - Added fallback module resolution to check `../frontend/node_modules/ws` if root `require('ws')` is not found, ensuring CDP console monitoring functions reliably when running `nexus start`.

---

## Verification Results

1. **Frontend Type Check**:
   - `npx tsc --noEmit`: **0 errors** (clean).
2. **Frontend Test Suite**:
   - `npm test -- --run` (Vitest): **189 / 189 tests passed** (clean).
3. **Rust Codebase Validation**:
   - `cargo check`: clean (0 errors, 23 warnings preserved).
   - `cargo test --lib -- --test-threads=1`: **1005 / 1005 tests passed** (clean, 0 failures, 6 ignored dev benches).
4. **Production Build**:
   - `npm run build`: built production dist in 5.43s.
   - `cargo build --release --features custom-protocol`: cleanly compiled fresh 95.8 MB release binary at `src-tauri/target/release/nexus.exe`.
