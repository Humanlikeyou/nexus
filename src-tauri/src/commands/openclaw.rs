use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// The old OpenClaw folder. READ-ONLY legacy fallback: nothing in this file may create, write or delete
/// anything under it (OpenClaw was removed and the folder moved aside).
fn openclaw_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".openclaw")
}

/// Where tasks, the content pipeline and skills are written now: `<data dir>/workspace`.
pub(crate) fn workspace_dir_in(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("workspace")
}

fn app_workspace() -> PathBuf {
    workspace_dir_in(&crate::app_paths::data_dir())
}

/// The file to read: the app-data copy first, else the legacy file when it exists, else the app-data path.
fn read_path(
    app_ws: &std::path::Path,
    legacy_ws: &std::path::Path,
    requested: Option<String>,
    name: &str,
) -> Result<PathBuf, String> {
    if requested.as_deref().is_some_and(|r| !r.trim().is_empty()) {
        let base = if app_ws.is_dir() { app_ws } else { legacy_ws };
        return workspace_file(base, requested, name);
    }
    if app_ws.join(name).exists() || !legacy_ws.join(name).exists() {
        Ok(app_ws.join(name))
    } else {
        Ok(legacy_ws.join(name))
    }
}

#[derive(Serialize, Deserialize, Default, Clone, specta::Type)]
pub struct Skill {
    pub name: String,
    pub status: String,
    pub description: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Serialize, Deserialize, Default, specta::Type)]
pub struct TaskItem {
    pub title: String,
    pub status: String,
    pub priority: Option<String>,
    pub tags: Vec<String>,
    /// From a `#venture:<id>` tag (written by the add flow, commands/adds.rs).
    #[serde(default)]
    pub venture: Option<String>,
}

#[derive(Serialize, Deserialize, Default, specta::Type)]
pub struct ContentPipelineItem {
    pub title: String,
    pub stage: String,
    pub content_type: String,
    pub notes: Option<String>,
    /// From a `#venture:<id>` tag (written by the add flow, commands/adds.rs).
    #[serde(default)]
    pub venture: Option<String>,
}

/// Largest file (tasks, pipeline, one skill) read into memory.
const MAX_READ_BYTES: u64 = 1024 * 1024;
/// Largest skill body `save_skill` will write.
const MAX_SKILL_BYTES: usize = 256 * 1024;

/// Read a UTF-8 file, refusing anything over `max` bytes (checked before and while reading).
fn read_capped(path: &std::path::Path, max: u64) -> Result<String, String> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map(|m| m.len()).unwrap_or(0) > max {
        return Err(format!("file is larger than {max} bytes"));
    }
    let mut buf = String::new();
    file.take(max + 1)
        .read_to_string(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > max {
        return Err(format!("file is larger than {max} bytes"));
    }
    Ok(buf)
}

/// A UI-supplied workspace file must be an existing `.md` inside ~/.openclaw/workspace
/// (after resolving symlinks); `None` means the default file there.
fn workspace_file(
    workspace: &std::path::Path,
    requested: Option<String>,
    default_name: &str,
) -> Result<PathBuf, String> {
    let Some(raw) = requested.filter(|s| !s.trim().is_empty()) else {
        return Ok(workspace.join(default_name));
    };
    let candidate = PathBuf::from(raw.trim());
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    let base = workspace
        .canonicalize()
        .map_err(|_| "Workspace not found".to_string())?;
    let resolved = candidate
        .canonicalize()
        .map_err(|_| "Workspace file not found".to_string())?;
    if !resolved.starts_with(&base)
        || !resolved.is_file()
        || resolved.extension().and_then(|e| e.to_str()) != Some("md")
    {
        return Err("Only .md files inside the workspace can be read".to_string());
    }
    Ok(resolved)
}

#[tauri::command]
#[specta::specta]
pub fn read_skills() -> Result<Vec<Skill>, String> {
    let mut skills = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Helper: load .md skills from a directory
    let load_from = |dir: &std::path::Path,
                     skills: &mut Vec<Skill>,
                     seen: &mut std::collections::HashSet<String>| {
        if !dir.exists() {
            return;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !seen.insert(name.clone()) {
                    continue;
                }
                // Oversized or unreadable skill files are skipped, not loaded.
                let Ok(content) = read_capped(&path, MAX_READ_BYTES) else {
                    continue;
                };
                // Extract first line after "# " as description
                let description = content
                    .lines()
                    .find(|l| l.starts_with("# "))
                    .map(|l| l.trim_start_matches("# ").to_string())
                    .unwrap_or_default();
                skills.push(Skill {
                    name,
                    status: "active".to_string(),
                    description,
                    content,
                });
            }
        }
    };

    // 1. Project skills: config/skills/ (walk up from binary to find project root)
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.as_path();
        while let Some(parent) = dir.parent() {
            let candidate = parent.join("config").join("skills");
            if candidate.exists() {
                load_from(&candidate, &mut skills, &mut seen);
                break;
            }
            dir = parent;
        }
    }
    // Fallback: well-known project path
    if let Some(home) = dirs::home_dir() {
        let fallback = home.join("projects/nexus/config/skills");
        if fallback.exists() {
            load_from(&fallback, &mut skills, &mut seen);
        }
    }

    // 2. Learned skills: <data dir>/workspace/skills first, then the legacy ~/.openclaw/skills (read-only).
    load_from(&app_workspace().join("skills"), &mut skills, &mut seen);
    load_from(&openclaw_dir().join("skills"), &mut skills, &mut seen);

    Ok(skills)
}

/// Write or update a skill file in `<data dir>/workspace/skills/` (never ~/.openclaw).
/// Used by the skills self-improvement loop to create/update learned skills.
#[tauri::command]
#[specta::specta]
pub fn save_skill(name: String, content: String) -> Result<(), String> {
    save_skill_in(&app_workspace().join("skills"), &name, content)
}

fn save_skill_in(dir: &std::path::Path, name: &str, content: String) -> Result<(), String> {
    if content.len() > MAX_SKILL_BYTES {
        return Err(format!("Skill is larger than {MAX_SKILL_BYTES} bytes"));
    }
    let path = skill_path(dir, name)?;
    crate::app_paths::create_private_dir_all(dir).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&path, content).map_err(|e| e.to_string())
}

/// Map a user-facing skill name ("Daily Brief") to `<dir>/daily-brief.md`,
/// rejecting anything that could escape the skills directory.
fn skill_path(dir: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    let stem = name.trim().replace(' ', "-").to_lowercase();
    super::path_safety::validate_name(&stem)?;
    super::path_safety::safe_join(dir, &format!("{stem}.md"))
}

/// Delete a skill file from `<data dir>/workspace/skills/`. A legacy skill is read-only: it reports false.
#[tauri::command]
#[specta::specta]
pub fn delete_skill(name: String) -> Result<bool, String> {
    delete_skill_in(&app_workspace().join("skills"), &name)
}

fn delete_skill_in(dir: &std::path::Path, name: &str) -> Result<bool, String> {
    let path = skill_path(dir, name)?;
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
#[specta::specta]
pub fn read_tasks(workspace_path: Option<String>) -> Result<Vec<TaskItem>, String> {
    let path = read_path(
        &app_workspace(),
        &openclaw_dir().join("workspace"),
        workspace_path,
        "TASKS.md",
    )?;
    let content = read_capped(&path, MAX_READ_BYTES).unwrap_or_default();
    let tasks = parse_tasks_md(&content);
    Ok(tasks)
}

pub(crate) fn parse_tasks_md(content: &str) -> Vec<TaskItem> {
    let mut tasks = Vec::new();
    let mut current_status = "backlog".to_string();

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect section headers
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let header = trimmed.trim_start_matches('#').trim().to_lowercase();
            if header.contains("active")
                || header.contains("sprint")
                || header.contains("in progress")
            {
                current_status = "active".to_string();
            } else if header.contains("backlog") || header.contains("todo") {
                current_status = "backlog".to_string();
            } else if header.contains("waiting") || header.contains("blocked") {
                current_status = "waiting".to_string();
            } else if header.contains("done") || header.contains("completed") {
                current_status = "done".to_string();
            }
        }

        // Parse task items (- [ ] or - [x] format)
        if trimmed.starts_with("- [") {
            let checked = trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]");
            let title = trimmed
                .trim_start_matches("- [x] ")
                .trim_start_matches("- [X] ")
                .trim_start_matches("- [ ] ")
                .to_string();

            let status = if checked {
                "done".to_string()
            } else {
                current_status.clone()
            };

            // Extract tags (words starting with #)
            let tags: Vec<String> = title
                .split_whitespace()
                .filter(|w| w.starts_with('#'))
                .map(|w| w.trim_start_matches('#').to_string())
                .collect();

            let venture = tags
                .iter()
                .find_map(|t| t.strip_prefix("venture:"))
                .map(str::to_string);
            tasks.push(TaskItem {
                title: title.split('#').next().unwrap_or(&title).trim().to_string(),
                status,
                priority: None,
                tags,
                venture,
            });
        }
    }

    if tasks.is_empty() {
        // Placeholder tasks
        tasks = vec![
            TaskItem {
                venture: None,
                title: "Launch TaiyoSkin landing page".to_string(),
                status: "active".to_string(),
                priority: Some("high".to_string()),
                tags: vec!["launch".to_string()],
            },
            TaskItem {
                venture: None,
                title: "Design product photography shoot".to_string(),
                status: "active".to_string(),
                priority: Some("medium".to_string()),
                tags: vec!["content".to_string()],
            },
            TaskItem {
                venture: None,
                title: "Set up Stripe payments".to_string(),
                status: "backlog".to_string(),
                priority: Some("high".to_string()),
                tags: vec!["payments".to_string()],
            },
            TaskItem {
                venture: None,
                title: "Create Instagram content calendar".to_string(),
                status: "backlog".to_string(),
                priority: Some("medium".to_string()),
                tags: vec!["content".to_string(), "social".to_string()],
            },
            TaskItem {
                venture: None,
                title: "Research shipping providers".to_string(),
                status: "waiting".to_string(),
                priority: Some("low".to_string()),
                tags: vec!["logistics".to_string()],
            },
            TaskItem {
                venture: None,
                title: "Brand identity guidelines".to_string(),
                status: "done".to_string(),
                priority: Some("high".to_string()),
                tags: vec!["design".to_string()],
            },
        ];
    }

    tasks
}

#[tauri::command]
#[specta::specta]
pub fn read_content_pipeline(
    workspace_path: Option<String>,
) -> Result<Vec<ContentPipelineItem>, String> {
    let path = read_path(
        &app_workspace(),
        &openclaw_dir().join("workspace"),
        workspace_path,
        "CONTENT-PIPELINE.md",
    )?;
    let content = read_capped(&path, MAX_READ_BYTES).unwrap_or_default();
    let items = parse_content_pipeline(&content);
    Ok(items)
}

pub(crate) fn parse_content_pipeline(content: &str) -> Vec<ContentPipelineItem> {
    let mut items = Vec::new();
    let mut current_stage = "ideation".to_string();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let header = trimmed.trim_start_matches('#').trim().to_lowercase();
            if header.contains("idea") {
                current_stage = "ideation".to_string();
            } else if header.contains("draft") || header.contains("writing") {
                current_stage = "drafting".to_string();
            } else if header.contains("review") || header.contains("edit") {
                current_stage = "review".to_string();
            } else if header.contains("ready") || header.contains("scheduled") {
                current_stage = "scheduled".to_string();
            } else if header.contains("published") || header.contains("live") {
                current_stage = "published".to_string();
            }
        }

        if trimmed.starts_with("- ") && !trimmed.starts_with("- [") {
            let line = trimmed.trim_start_matches("- ");
            let tag = |key: &str| {
                line.split_whitespace()
                    .filter_map(|w| w.strip_prefix('#'))
                    .find_map(|t| t.strip_prefix(key))
                    .map(str::to_string)
            };
            items.push(ContentPipelineItem {
                title: line.split(" #").next().unwrap_or(line).trim().to_string(),
                stage: current_stage.clone(),
                content_type: tag("type:").unwrap_or_else(|| "post".to_string()),
                notes: None,
                venture: tag("venture:"),
            });
        }
    }

    if items.is_empty() {
        items = vec![
            ContentPipelineItem {
                venture: None,
                title: "Skincare routine for melanin-rich skin".to_string(),
                stage: "ideation".to_string(),
                content_type: "blog".to_string(),
                notes: None,
            },
            ContentPipelineItem {
                venture: None,
                title: "Morning glow tutorial — TikTok".to_string(),
                stage: "drafting".to_string(),
                content_type: "video".to_string(),
                notes: None,
            },
            ContentPipelineItem {
                venture: None,
                title: "Ingredient spotlight: Niacinamide".to_string(),
                stage: "review".to_string(),
                content_type: "carousel".to_string(),
                notes: None,
            },
            ContentPipelineItem {
                venture: None,
                title: "Product launch announcement".to_string(),
                stage: "scheduled".to_string(),
                content_type: "post".to_string(),
                notes: Some("Scheduled for next Monday".to_string()),
            },
            ContentPipelineItem {
                venture: None,
                title: "Brand story — our 'why'".to_string(),
                stage: "published".to_string(),
                content_type: "blog".to_string(),
                notes: None,
            },
        ];
    }

    items
}

#[cfg(test)]
mod input_tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("nexus-oc-{tag}-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn read_capped_refuses_oversized_files() {
        let d = tmp("cap");
        let small = d.join("a.md");
        fs::write(&small, "hello").unwrap();
        assert_eq!(read_capped(&small, 5).unwrap(), "hello");
        assert!(read_capped(&small, 4).is_err());
        assert!(read_capped(&d.join("missing.md"), 10).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn workspace_file_stays_inside_the_workspace() {
        let root = tmp("ws");
        let ws = root.join("workspace");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("TASKS.md"), "- [ ] a").unwrap();
        fs::write(ws.join("notes.txt"), "x").unwrap();
        fs::write(root.join("secret.md"), "x").unwrap();

        assert_eq!(
            workspace_file(&ws, None, "TASKS.md").unwrap(),
            ws.join("TASKS.md")
        );
        assert_eq!(
            workspace_file(&ws, Some("  ".into()), "TASKS.md").unwrap(),
            ws.join("TASKS.md")
        );
        assert_eq!(
            workspace_file(&ws, Some("TASKS.md".into()), "x").unwrap(),
            ws.join("TASKS.md")
        );
        let abs = ws.join("TASKS.md").to_string_lossy().to_string();
        assert!(workspace_file(&ws, Some(abs), "x").is_ok());
        for bad in [
            "../secret.md".to_string(),
            root.join("secret.md").to_string_lossy().to_string(),
            "/etc/hosts".to_string(),
            "notes.txt".to_string(),
            "missing.md".to_string(),
        ] {
            assert!(
                workspace_file(&ws, Some(bad.clone()), "x").is_err(),
                "{bad}"
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("secret.md"), ws.join("link.md")).unwrap();
            assert!(
                workspace_file(&ws, Some("link.md".into()), "x").is_err(),
                "symlink escape"
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn save_skill_refuses_oversized_content_before_touching_disk() {
        let err = save_skill("too-big".into(), "x".repeat(MAX_SKILL_BYTES + 1)).unwrap_err();
        assert!(err.contains("larger"), "{err}");
    }

    #[test]
    fn reads_prefer_app_data_then_fall_back_to_the_legacy_file() {
        let root = tmp("rp");
        let (app, legacy) = (root.join("app"), root.join("legacy"));
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&legacy).unwrap();
        // Neither has it: the app-data path (never the legacy one).
        assert_eq!(
            read_path(&app, &legacy, None, "TASKS.md").unwrap(),
            app.join("TASKS.md")
        );
        fs::write(legacy.join("TASKS.md"), "- [ ] old").unwrap();
        assert_eq!(
            read_path(&app, &legacy, None, "TASKS.md").unwrap(),
            legacy.join("TASKS.md")
        );
        fs::write(app.join("TASKS.md"), "- [ ] new").unwrap();
        assert_eq!(
            read_path(&app, &legacy, None, "TASKS.md").unwrap(),
            app.join("TASKS.md")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn skills_are_written_and_deleted_in_app_data_only() {
        let root = tmp("sk");
        let (app, legacy) = (
            root.join("app").join("skills"),
            root.join("home").join(".openclaw").join("skills"),
        );
        save_skill_in(&app, "Daily Brief", "# brief".into()).unwrap();
        assert_eq!(
            fs::read_to_string(app.join("daily-brief.md")).unwrap(),
            "# brief"
        );
        assert!(!legacy.exists() && !legacy.parent().unwrap().exists());
        // A legacy skill is never deleted: delete only looks in the app-data folder.
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("old.md"), "# old").unwrap();
        assert!(!delete_skill_in(&app, "old").unwrap());
        assert!(legacy.join("old.md").exists());
        assert!(delete_skill_in(&app, "daily-brief").unwrap());
        let _ = fs::remove_dir_all(&root);
    }
}
