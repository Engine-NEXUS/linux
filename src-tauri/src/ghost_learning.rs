//! Local, structural Ghost control learning. A rule binds a spoken alias to
//! one accessible control in one app build. It never stores a screen image,
//! user-entered value, message, password, or pixel coordinate.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LearnedControl {
    alias: String,
    exe: String,
    app_path_hash: String,
    app_size: u64,
    app_modified_secs: u64,
    automation_id: String,
    control_type: String,
    name_hash: String,
}

fn store_path(dir: &Path) -> std::path::PathBuf {
    dir.join("ghost").join("learned_controls.json")
}

fn load(dir: &Path) -> Result<Vec<LearnedControl>, String> {
    let path = store_path(dir);
    let bytes = match std::fs::read(&path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Could not read learned controls: {e}")),
    };
    serde_json::from_slice(&bytes)
        .map_err(|_| "The learned-control store is damaged; I won't guess.".into())
}

fn save(dir: &Path, controls: &[LearnedControl]) -> Result<(), String> {
    if controls.len() > 100 {
        return Err("The Ghost learning store is full.".into());
    }
    let path = store_path(dir);
    let parent = path.parent().ok_or("Invalid Ghost store path.")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("Could not create Ghost store: {e}"))?;
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(controls).map_err(|e| e.to_string())?;
    std::fs::write(&temp, bytes).map_err(|e| format!("Could not write Ghost store: {e}"))?;
    std::fs::rename(&temp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("Could not update Ghost store: {e}")
    })
}

#[cfg(target_os = "windows")]
mod win {
    use super::{load, save, LearnedControl};
    use sha2::{Digest, Sha256};
    use std::path::Path;
    use std::time::UNIX_EPOCH;
    use uiautomation::controls::ControlType;
    use uiautomation::types::{Handle, Point, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::UIAutomation;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    fn app_identity() -> Result<(String, String, u64, u64), String> {
        let exe = crate::browser_url::foreground_process_name()
            .ok_or_else(|| "I can't identify this app.".to_string())?;
        let path = crate::browser_url::foreground_process_path()
            .ok_or_else(|| "I can't identify this app build.".to_string())?;
        let app_path_hash = format!(
            "{:x}",
            Sha256::digest(path.to_string_lossy().to_lowercase().as_bytes())
        );
        let meta =
            std::fs::metadata(path).map_err(|_| "I can't read this app build.".to_string())?;
        let modified = meta
            .modified()
            .map_err(|_| "I can't read this app version.".to_string())?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "I can't read this app version.".to_string())?
            .as_secs();
        Ok((exe, app_path_hash, meta.len(), modified))
    }

    fn safe_foreground() -> Result<(UIAutomation, isize), String> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0 == 0 {
            return Err("I can't identify the foreground window.".into());
        }
        let exe = crate::browser_url::foreground_process_name()
            .ok_or_else(|| "I can't identify this app.".to_string())?;
        let title = crate::browser_url::foreground_window_title().unwrap_or_default();
        if crate::shortcuts::exec::is_protected_window(&exe, &title) {
            return Err("That app is protected.".into());
        }
        if crate::live::commands::window::window_process_elevated(hwnd)
            && !crate::live::commands::window::our_process_elevated()
        {
            return Err("That app runs as administrator.".into());
        }
        let automation = UIAutomation::new().map_err(|e| e.to_string())?;
        Ok((automation, hwnd.0))
    }

    fn name_hash(name: &str) -> String {
        format!("{:x}", Sha256::digest(name.as_bytes()))
    }

    fn is_actionable(kind: &ControlType) -> bool {
        matches!(
            kind,
            ControlType::Button
                | ControlType::Hyperlink
                | ControlType::TabItem
                | ControlType::MenuItem
                | ControlType::ListItem
                | ControlType::CheckBox
                | ControlType::RadioButton
                | ControlType::SplitButton
                | ControlType::ComboBox
                | ControlType::Edit
        )
    }

    /// The user points to a control and says "remember this button as X".
    /// This only observes and saves an accessible identity; it never clicks.
    pub fn teach(dir: &Path, alias: &str) -> Result<String, String> {
        let (automation, hwnd) = safe_foreground()?;
        let (exe, app_path_hash, app_size, app_modified_secs) = app_identity()?;
        let (x, y) = crate::live::commands::mouse::current_pos()
            .ok_or_else(|| "I can't read the pointer position.".to_string())?;
        let el = automation
            .element_from_point(Point::new(x, y))
            .map_err(|e| e.to_string())?;
        if !el.is_enabled().unwrap_or(false)
            || el.is_offscreen().unwrap_or(true)
            || el.is_password().unwrap_or(true)
        {
            return Err("That control is unavailable or protected.".into());
        }
        let name = el.get_name().map_err(|e| e.to_string())?;
        if name.trim().is_empty()
            || crate::ghost_center::is_risky_target(&name)
            || crate::ghost_center::is_ambiguous_target(&name)
        {
            return Err("That control is too ambiguous or risky to learn.".into());
        }
        let kind = el.get_control_type().map_err(|e| e.to_string())?;
        if !is_actionable(&kind) {
            return Err("Point to an accessible button or control and try again.".into());
        }
        let id = el.get_automation_id().map_err(|e| e.to_string())?;
        if id.is_empty() || id.len() > 80 || id.contains('@') || id.chars().any(char::is_whitespace)
        {
            return Err("This app does not expose a safe, stable control ID.".into());
        }
        let root = automation
            .element_from_handle(Handle::from(hwnd))
            .map_err(|e| e.to_string())?;
        let cond = automation
            .create_property_condition(UIProperty::AutomationId, Variant::from(id.clone()), None)
            .map_err(|e| e.to_string())?;
        let matches = root
            .find_all(TreeScope::Descendants, &cond)
            .map_err(|e| e.to_string())?;
        let selected_id = el.get_runtime_id().map_err(|e| e.to_string())?;
        if matches.len() != 1
            || matches[0].get_runtime_id().map_err(|e| e.to_string())? != selected_id
        {
            return Err("This control ID is not unique in the current window.".into());
        }
        if unsafe { GetForegroundWindow() }.0 != hwnd || crate::ghost::stop_requested() {
            return Err("The window changed while I was learning.".into());
        }
        let mut controls = load(dir)?;
        controls
            .retain(|c| !(c.alias == alias && c.exe == exe && c.app_path_hash == app_path_hash));
        controls.push(LearnedControl {
            alias: alias.to_string(),
            exe,
            app_path_hash,
            app_size,
            app_modified_secs,
            automation_id: id,
            control_type: format!("{kind:?}"),
            name_hash: name_hash(&name),
        });
        save(dir, &controls)?;
        Ok(format!("Learned {alias} for this app"))
    }

    /// Find the learned control again in the *current* foreground window.
    /// A changed app binary or changed accessible identity invalidates it.
    pub fn resolve(dir: &Path, alias: &str) -> Result<(String, String), String> {
        let (automation, hwnd) = safe_foreground()?;
        let (exe, app_path_hash, app_size, app_modified_secs) = app_identity()?;
        let controls = load(dir)?;
        let rule = controls
            .iter()
            .find(|c| c.alias == alias && c.exe == exe && c.app_path_hash == app_path_hash)
            .ok_or_else(|| format!("I haven't learned {alias} in this app."))?;
        if rule.app_size != app_size || rule.app_modified_secs != app_modified_secs {
            return Err(format!(
                "{exe} changed since I learned {alias}; please teach it again."
            ));
        }
        let root = automation
            .element_from_handle(Handle::from(hwnd))
            .map_err(|e| e.to_string())?;
        let cond = automation
            .create_property_condition(
                UIProperty::AutomationId,
                Variant::from(rule.automation_id.clone()),
                None,
            )
            .map_err(|e| e.to_string())?;
        let matches = root
            .find_all(TreeScope::Descendants, &cond)
            .map_err(|e| e.to_string())?;
        if matches.len() != 1 {
            return Err("The learned control is missing or ambiguous now.".into());
        }
        let el = &matches[0];
        if !el.is_enabled().unwrap_or(false)
            || el.is_offscreen().unwrap_or(true)
            || el.is_password().unwrap_or(true)
            || format!("{:?}", el.get_control_type().map_err(|e| e.to_string())?)
                != rule.control_type
        {
            return Err("The learned control is no longer safe to use.".into());
        }
        let name = el.get_name().map_err(|e| e.to_string())?;
        if name_hash(&name) != rule.name_hash
            || crate::ghost_center::is_risky_target(&name)
            || crate::ghost_center::is_ambiguous_target(&name)
        {
            return Err("The learned control changed; please teach it again.".into());
        }
        if unsafe { GetForegroundWindow() }.0 != hwnd || crate::ghost::stop_requested() {
            return Err("The window changed before I could use the learned control.".into());
        }
        Ok((name, rule.automation_id.clone()))
    }

    pub fn forget(dir: &Path, alias: &str) -> Result<String, String> {
        let (exe, app_path_hash, _, _) = app_identity()?;
        let mut controls = load(dir)?;
        let before = controls.len();
        controls
            .retain(|c| !(c.alias == alias && c.exe == exe && c.app_path_hash == app_path_hash));
        if controls.len() == before {
            return Err(format!("I haven't learned {alias} in this app."));
        }
        save(dir, &controls)?;
        Ok(format!("Forgot {alias} for this app"))
    }
}

#[cfg(target_os = "windows")]
pub use win::{forget, resolve, teach};

#[cfg(not(target_os = "windows"))]
pub fn teach(_dir: &Path, _alias: &str) -> Result<String, String> {
    Err("Ghost learning currently requires Windows.".into())
}
#[cfg(not(target_os = "windows"))]
pub fn resolve(_dir: &Path, _alias: &str) -> Result<(String, String), String> {
    Err("Ghost learning currently requires Windows.".into())
}
#[cfg(not(target_os = "windows"))]
pub fn forget(_dir: &Path, _alias: &str) -> Result<String, String> {
    Err("Ghost learning currently requires Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damaged_store_fails_closed() {
        let dir =
            std::env::temp_dir().join(format!("nexus-ghost-learning-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ghost")).unwrap();
        std::fs::write(store_path(&dir), b"not-json").unwrap();
        assert!(load(&dir).is_err());
        std::fs::remove_file(store_path(&dir)).unwrap();
        std::fs::remove_dir(dir.join("ghost")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn learned_store_roundtrips_and_replaces_without_coordinates() {
        let dir = std::env::temp_dir().join(format!(
            "nexus-ghost-learning-roundtrip-{}",
            std::process::id()
        ));
        let rule = LearnedControl {
            alias: "settings".into(),
            exe: "notepad.exe".into(),
            app_path_hash: "app-path-hash".into(),
            app_size: 42,
            app_modified_secs: 77,
            automation_id: "SettingsButton".into(),
            control_type: "Button".into(),
            name_hash: "hashed".into(),
        };
        save(&dir, &[rule.clone()]).unwrap();
        assert_eq!(load(&dir).unwrap()[0].automation_id, "SettingsButton");
        let updated = LearnedControl {
            alias: "preferences".into(),
            ..rule
        };
        save(&dir, &[updated]).unwrap();
        let found = load(&dir).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].alias, "preferences");
        std::fs::remove_file(store_path(&dir)).unwrap();
        std::fs::remove_dir(dir.join("ghost")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
