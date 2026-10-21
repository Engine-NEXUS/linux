//! Screen & workspace grounding for coding projects.
//!
//! Inspects the user's active window / foreground app to detect which code
//! project they are working on, determines the appropriate development server
//! command (Vite, Next.js, Cargo, Python, etc.), and launches localhost.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectType {
    Node,
    Rust,
    Python,
    Go,
    Static,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub name: String,
    pub path: PathBuf,
    pub project_type: ProjectType,
    pub dev_command: String,
    pub default_port: u16,
}

/// Detect project type and development command for a given directory.
pub fn inspect_project_dir(dir: &Path) -> (ProjectType, String, u16) {
    // 1. Node.js (package.json)
    let pkg_json = dir.join("package.json");
    if pkg_json.is_file() {
        if let Ok(content) = std::fs::read_to_string(&pkg_json) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                let scripts = v.get("scripts");
                let has_dev = scripts.and_then(|s| s.get("dev")).is_some();
                let has_start = scripts.and_then(|s| s.get("start")).is_some();

                // Check lockfile for package manager preference
                let pm = if dir.join("pnpm-lock.yaml").exists() {
                    "pnpm"
                } else if dir.join("yarn.lock").exists() {
                    "yarn"
                } else if dir.join("bun.lockb").exists() || dir.join("bun.lock").exists() {
                    "bun"
                } else {
                    "npm"
                };

                // Check dependencies for typical port (Vite vs Next vs CRA)
                let deps = v.get("dependencies");
                let dev_deps = v.get("devDependencies");
                let is_vite = deps.and_then(|d| d.get("vite")).is_some()
                    || dev_deps.and_then(|d| d.get("vite")).is_some();
                let is_next = deps.and_then(|d| d.get("next")).is_some();

                let port = if is_next {
                    3000
                } else if is_vite {
                    5173
                } else {
                    3000
                };

                let cmd = if has_dev {
                    if pm == "npm" {
                        "npm run dev".to_string()
                    } else {
                        format!("{} dev", pm)
                    }
                } else if has_start {
                    if pm == "npm" {
                        "npm start".to_string()
                    } else {
                        format!("{} start", pm)
                    }
                } else {
                    "npm run dev".to_string()
                };

                return (ProjectType::Node, cmd, port);
            }
        }
        return (ProjectType::Node, "npm run dev".to_string(), 5173);
    }

    // 2. Rust (Cargo.toml)
    let cargo_toml = dir.join("Cargo.toml");
    if cargo_toml.is_file() {
        return (ProjectType::Rust, "cargo run".to_string(), 8080);
    }

    // 3. Python (manage.py, main.py, app.py)
    if dir.join("manage.py").is_file() {
        return (ProjectType::Python, "python manage.py runserver".to_string(), 8000);
    }
    if dir.join("main.py").is_file() {
        return (ProjectType::Python, "python main.py".to_string(), 8000);
    }
    if dir.join("app.py").is_file() {
        return (ProjectType::Python, "python app.py".to_string(), 5000);
    }

    // 4. Go (go.mod)
    if dir.join("go.mod").is_file() {
        return (ProjectType::Go, "go run .".to_string(), 8080);
    }

    // 5. Static HTML (index.html)
    if dir.join("index.html").is_file() {
        return (ProjectType::Static, "npx serve .".to_string(), 3000);
    }

    (ProjectType::Unknown, "npm run dev".to_string(), 3000)
}

/// Parse project name from window title string.
/// Formats handled:
/// - "index.tsx - ULTRON - Visual Studio Code"
/// - "nexus - Cursor"
/// - "src/main.rs - MyProject - Trae"
/// - "OpenCode - ProjectX"
pub fn extract_project_name_from_title(title: &str) -> Option<String> {
    let t = title.trim();
    if t.is_empty() {
        return None;
    }

    let parts: Vec<&str> = t.split(" - ").map(|s| s.trim()).collect();
    if parts.len() >= 3 {
        let proj = parts[parts.len() - 2];
        if !proj.is_empty() && !is_known_ide_suffix(proj) {
            return Some(proj.to_string());
        }
    } else if parts.len() == 2 {
        if is_known_ide_suffix(parts[1]) {
            return Some(parts[0].to_string());
        } else if is_known_ide_suffix(parts[0]) {
            return Some(parts[1].to_string());
        }
    }

    None
}

fn is_known_ide_suffix(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.contains("visual studio")
        || lower.contains("cursor")
        || lower.contains("trae")
        || lower.contains("windsurf")
        || lower.contains("opencode")
        || lower.contains("sublime")
}

/// Find a matching directory for a project name on the system.
pub fn find_project_directory(project_name: &str) -> Option<PathBuf> {
    let norm = project_name.trim().to_lowercase();
    let search_roots = [
        PathBuf::from("c:\\PROJECTS"),
        PathBuf::from("c:\\PROJECTS\\MAIN Projects"),
        PathBuf::from("c:\\PROJECTS\\MINOR Projects"),
        PathBuf::from("."),
    ];

    for root in &search_roots {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        let name = entry.file_name().to_string_lossy().to_lowercase();
                        if name == norm || name.contains(&norm) {
                            return Some(entry.path());
                        }
                    }
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let name = cwd.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if name == norm || norm.is_empty() {
            return Some(cwd);
        }
    }

    None
}

/// Get active project context from foreground window.
pub fn detect_active_project() -> Option<ProjectContext> {
    let title = crate::browser_url::foreground_window_title().unwrap_or_default();
    let proj_name = extract_project_name_from_title(&title);

    let proj_dir = match &proj_name {
        Some(name) => find_project_directory(name),
        // No project named in the window title: the process directory is only
        // a project if it looks like one. The installed app runs from its own
        // folder, and `npm run dev` there would be nonsense.
        None => std::env::current_dir()
            .ok()
            .filter(|d| inspect_project_dir(d).0 != ProjectType::Unknown),
    }?;

    let final_name = proj_name.unwrap_or_else(|| {
        proj_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "current project".to_string())
    });

    let (ptype, dev_cmd, port) = inspect_project_dir(&proj_dir);

    Some(ProjectContext {
        name: final_name,
        path: proj_dir,
        project_type: ptype,
        dev_command: dev_cmd,
        default_port: port,
    })
}

/// Launch the development server for the project in the background.
pub fn launch_localhost(project: &ProjectContext) -> Result<String, String> {
    let dir = &project.path;
    let cmd = &project.dev_command;

    tracing::info!("launch_localhost: starting '{}' in '{:?}'", cmd, dir);

    if project.project_type == ProjectType::Unknown {
        return Err("it has no package.json, Cargo.toml, go.mod or similar to start from".into());
    }
    if cmd.split_whitespace().next().is_none() {
        return Err("Empty dev command".into());
    }

    // `npm`, `pnpm`, `yarn` and `npx` are .cmd shims on Windows, which a bare
    // CreateProcess of "npm" cannot find: go through the shell there.
    #[cfg(windows)]
    let child = std::process::Command::new("cmd").args(["/C", cmd]).current_dir(dir).spawn();
    #[cfg(not(windows))]
    let child = {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        std::process::Command::new(parts[0]).args(&parts[1..]).current_dir(dir).spawn()
    };

    match child {
        Ok(_) => Ok(format!(
            "Started dev server for {} using `{}` on localhost:{}",
            project.name, project.dev_command, project.default_port
        )),
        Err(e) => Err(format!("Failed to start dev server: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_project_name() {
        assert_eq!(
            extract_project_name_from_title("index.tsx - ULTRON - Visual Studio Code"),
            Some("ULTRON".to_string())
        );
        assert_eq!(
            extract_project_name_from_title("nexus - Cursor"),
            Some("nexus".to_string())
        );
        assert_eq!(
            extract_project_name_from_title("main.rs - MyProject - Trae"),
            Some("MyProject".to_string())
        );
        assert_eq!(
            extract_project_name_from_title("OpenCode - WebApp"),
            Some("WebApp".to_string())
        );
    }

    #[test]
    fn test_inspect_current_dir() {
        let (ptype, _cmd, _port) = inspect_project_dir(Path::new("."));
        // Cwd has package.json and Cargo.toml; if package.json is in root or src-tauri
        assert!(ptype == ProjectType::Node || ptype == ProjectType::Rust || ptype == ProjectType::Unknown);
    }

    #[test]
    fn unrecognised_directory_is_not_started() {
        let dir = std::env::temp_dir().join("nexus_grounding_empty_dir_test");
        let _ = std::fs::create_dir_all(&dir);
        let (t, cmd, port) = inspect_project_dir(&dir);
        assert_eq!(t, ProjectType::Unknown);
        let ctx = ProjectContext { name: "x".into(), path: dir, project_type: t, dev_command: cmd, default_port: port };
        assert!(launch_localhost(&ctx).is_err(), "must not run `npm run dev` in a folder that is not a project");
    }
}
