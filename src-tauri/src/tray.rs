//! System tray: show/hide, pause/resume, settings, quit.
//! Keeps the app alive after window hide.

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, MenuItemKind, CheckMenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};
#[cfg(not(target_os = "windows"))]
use tauri_plugin_autostart::ManagerExt;

use crate::meeting_detect::MeetingState;

pub fn setup<R: Runtime>(app: &AppHandle<R>) -> Result<(), tauri::Error> {
    let show = MenuItem::with_id(app, "show", "Show Assistant", true, None::<&str>)?;
    let separator1 = PredefinedMenuItem::separator(app)?;
    let pause = MenuItem::with_id(app, "pause", "Pause NEXUS", true, None::<&str>)?;
    let separator2 = PredefinedMenuItem::separator(app)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", "Start at Login", true, true, None::<&str>)?;
    let separator3 = PredefinedMenuItem::separator(app)?;
    let settings = MenuItem::with_id(app, "settings", "Command Hub…", true, None::<&str>)?;
    let full_settings = MenuItem::with_id(app, "full_settings", "Command Hub (Full Window)", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit NEXUS", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[
        &show,
        &separator1,
        &pause,
        &separator2,
        &autostart,
        &separator3,
        &settings,
        &full_settings,
        &quit,
    ])?;

    let handle = app.clone();
    TrayIconBuilder::with_id("NEXUS-tray")
        .icon(app.default_window_icon().cloned().unwrap())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, id| match id.id().as_ref() {
            "show" => {
                crate::window_manager::wake_orb_src(app, "tray-menu");
            }
            "pause" => {
                // Toggle manual pause via meeting state
                if let Some(state) = app.try_state::<std::sync::Arc<MeetingState>>() {
                    let now_paused = state.toggle_pause();
                    // Update menu item label
                    let new_label = if now_paused {
                        "Resume NEXUS"
                    } else {
                        "Pause NEXUS"
                    };
                    if let Some(MenuItemKind::MenuItem(mi)) =
                        app.menu().and_then(|m| m.get("pause"))
                    {
                        let _ = mi.set_text(new_label);
                    }
                    if now_paused {
                        tracing::info!("tray: NEXUS paused (manual)");
                    } else {
                        tracing::info!("tray: NEXUS resumed (manual)");
                    }
                }
            }
            "settings" => {
                // Open the settings sidebar (liquid-glass, 720x1000, always-on-top)
                let app_clone = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::commands::show_settings_sidebar(app_clone).await {
                        tracing::warn!("tray: failed to open settings sidebar: {e}");
                    }
                });
            }
            "full_settings" => {
                // Open the full settings window (SettingsApp — save/autostart/
                // setup-wizard/clear-transcript). Covered by main-cap since
                // the capability fix; the window had zero openers before.
                if let Err(e) = crate::commands::open_settings_window(app.clone()) {
                    tracing::warn!("tray: failed to open settings window: {e}");
                }
            }
            "autostart" => {
                // Toggle the autostart check menu item
                if let Some(MenuItemKind::Check(mi)) =
                    app.menu().and_then(|m| m.get("autostart"))
                {
                    let new_state = !mi.is_checked().unwrap_or(false);
                    let _ = mi.set_checked(new_state);
                    // Call the set_autostart command logic directly
                    let exe_path = std::env::current_exe()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !exe_path.is_empty() {
                        #[cfg(target_os = "windows")]
                        {
                            if new_state {
                                let ps_script = format!(
                                    r#"$exe = '{}';
                                    $user = [Security.Principal.WindowsIdentity]::GetCurrent().Name;
                                    $action = New-ScheduledTaskAction -Execute $exe -Argument '--background';
                                    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $user;
                                    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Seconds 0);
                                    $result = Register-ScheduledTask -TaskName 'NEXUS' -Action $action -Trigger $trigger -Settings $settings -User $user -Force;
                                    if ($result) {{ Write-Output 'NEXUS_TASK_OK' }} else {{ Write-Output 'NEXUS_TASK_FAIL' }}"#,
                                    exe_path
                                );
                                let _ = std::process::Command::new("powershell")
                                    .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
                                    .creation_flags(0x08000000)
                                    .output();
                                tracing::info!("tray: autostart enabled via tray toggle");
                            } else {
                                let _ = std::process::Command::new("powershell")
                                    .args(["-NoProfile", "-NonInteractive", "-Command",
                                        "Unregister-ScheduledTask -TaskName 'NEXUS' -Confirm:$false -ErrorAction SilentlyContinue"])
                                    .creation_flags(0x08000000)
                                    .output();
                                tracing::info!("tray: autostart disabled via tray toggle");
                            }
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            let autolaunch = app.autolaunch();
                            if new_state {
                                let _ = autolaunch.enable();
                            } else {
                                let _ = autolaunch.disable();
                            }
                        }
                    }
                    // Also persist the setting to settings.json
                    if let Some(state) = app.try_state::<std::sync::Arc<MeetingState>>() {
                        let _ = state; // just to avoid unused warning
                    }
                    if let Some(dir) = app.path().app_data_dir().ok() {
                        let settings_path = dir.join("settings.json");
                        if settings_path.exists() {
                            if let Ok(content) = std::fs::read_to_string(&settings_path) {
                                if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
                                    if let Some(obj) = json.as_object_mut() {
                                        obj.insert("autostart".to_string(), serde_json::json!(new_state));
                                        let _ = std::fs::write(&settings_path, serde_json::to_string_pretty(&json).unwrap_or_default());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "quit" => {
                crate::volume::force_restore_volume();
                app.exit(0)
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                let app = tray.app_handle();
                crate::window_manager::wake_orb_src(app, "tray-click");
            }
        })
        .build(app)?;

    // Sync the "Start at Login" checkbox with the actual OS state
    #[cfg(target_os = "windows")]
    {
        let result = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "Get-ScheduledTask -TaskName 'NEXUS' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty State"])
            .creation_flags(0x08000000)
            .output();
        let is_enabled = match result {
            Ok(out) => {
                let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
                state == "Ready" || state == "Running"
            }
            Err(_) => false,
        };
        if let Some(MenuItemKind::Check(mi)) =
            app.menu().and_then(|m| m.get("autostart"))
        {
            let _ = mi.set_checked(is_enabled);
        }
    }

    // Keep a reference so the compiler knows `handle` is used for future extension.
    let _ = handle;
    Ok(())
}
