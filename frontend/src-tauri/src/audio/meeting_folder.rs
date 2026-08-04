//! Naming, creation, and renaming of meeting recording folders.
//!
//! Layout: `<base>/<prefix><MeetingName>_<YYYY-MM-DD_HH-MM>/`
//!
//! The prefix comes from [`RecordingPreferences::folder_prefix`] and is applied
//! verbatim, so the user owns the separator (`Work_`, `[Client] `, `2026-q3-`).
//! The settings UI shows a live preview of the resulting name.
//!
//! [`RecordingPreferences::folder_prefix`]: super::recording_preferences::RecordingPreferences::folder_prefix

use anyhow::{anyhow, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};

use super::recording_preferences::{get_default_recordings_folder, RecordingPreferences};

/// Timestamp suffix appended to every new meeting folder.
const TIMESTAMP_FORMAT: &str = "%Y-%m-%d_%H-%M";

/// Upper bound on `_2`, `_3`, … suffixes tried when a folder name is taken.
const MAX_UNIQUE_ATTEMPTS: u32 = 1000;

/// Name used when sanitizing leaves nothing usable (e.g. the user typed "...").
const FALLBACK_NAME: &str = "Untitled Meeting";

/// Where new meeting folders go and how they are named, resolved from the
/// user's recording preferences once at the start of a recording or import.
#[derive(Debug, Clone)]
pub struct MeetingFolderConfig {
    /// Base recordings directory.
    pub base_folder: PathBuf,
    /// Optional prefix applied to the folder name.
    pub prefix: Option<String>,
}

impl Default for MeetingFolderConfig {
    fn default() -> Self {
        Self {
            base_folder: get_default_recordings_folder(),
            prefix: None,
        }
    }
}

impl MeetingFolderConfig {
    pub fn from_preferences(preferences: &RecordingPreferences) -> Self {
        Self {
            base_folder: preferences.save_folder.clone(),
            prefix: preferences
                .folder_prefix
                .as_ref()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty()),
        }
    }

    /// Create the folder for a meeting under this configuration.
    pub fn create(&self, meeting_name: &str, create_checkpoints_dir: bool) -> Result<PathBuf> {
        create_meeting_folder(
            &self.base_folder,
            meeting_name,
            self.prefix.as_deref(),
            create_checkpoints_dir,
        )
    }
}

/// Sanitize a string into a single safe path component.
///
/// Stricter than a plain character filter: it also rejects `.`/`..` and strips
/// trailing dots and spaces (illegal on Windows), so the result can never escape
/// its parent directory or produce an unopenable folder.
pub fn sanitize_component(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    // Windows silently drops trailing dots/spaces, which would make the path we
    // record in the database disagree with the folder actually on disk.
    let trimmed = replaced.trim().trim_end_matches(['.', ' ']).trim();

    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        FALLBACK_NAME.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Build the folder name for a new meeting: `<prefix><name>_<timestamp>`.
///
/// The prefix is sanitized but otherwise applied verbatim — no separator is
/// inserted, so `Work_` yields `Work_Standup_...` and `Work` yields `WorkStandup_...`.
pub fn build_folder_name(prefix: Option<&str>, meeting_name: &str, timestamp: &str) -> String {
    let prefix = prefix
        .map(sanitize_component)
        .filter(|p| p != FALLBACK_NAME)
        .unwrap_or_default();

    format!("{}{}_{}", prefix, sanitize_component(meeting_name), timestamp)
}

/// Resolve `folder_name` inside `base` to a path that does not exist yet.
///
/// Appends `_2`, `_3`, … on collision. Two meetings started in the same minute
/// with the same name would otherwise land in one folder and overwrite each
/// other's `audio.mp4`.
fn unique_folder_path(base: &Path, folder_name: &str) -> PathBuf {
    let candidate = base.join(folder_name);
    if !candidate.exists() {
        return candidate;
    }

    for n in 2..=MAX_UNIQUE_ATTEMPTS {
        let candidate = base.join(format!("{}_{}", folder_name, n));
        if !candidate.exists() {
            return candidate;
        }
    }

    // Astronomically unlikely; fall back to a timestamp that includes seconds.
    base.join(format!(
        "{}_{}",
        folder_name,
        Utc::now().format("%H-%M-%S%.3f")
    ))
}

/// Create a meeting folder and return its path.
///
/// Creates `<base_path>/<prefix><MeetingName>_<timestamp>/`, plus a
/// `.checkpoints/` subdirectory when `create_checkpoints_dir` is set (auto-save on).
///
/// # Arguments
/// * `base_path` - Base recordings directory
/// * `meeting_name` - Human name of the meeting
/// * `prefix` - Optional user-configured folder prefix
/// * `create_checkpoints_dir` - Whether to create `.checkpoints/`
pub fn create_meeting_folder(
    base_path: &Path,
    meeting_name: &str,
    prefix: Option<&str>,
    create_checkpoints_dir: bool,
) -> Result<PathBuf> {
    let timestamp = Utc::now().format(TIMESTAMP_FORMAT).to_string();
    let folder_name = build_folder_name(prefix, meeting_name, &timestamp);
    let meeting_folder = unique_folder_path(base_path, &folder_name);

    std::fs::create_dir_all(&meeting_folder)?;

    if create_checkpoints_dir {
        std::fs::create_dir_all(meeting_folder.join(".checkpoints"))?;
        log::info!(
            "Created meeting folder with checkpoints: {}",
            meeting_folder.display()
        );
    } else {
        log::info!(
            "Created meeting folder without checkpoints: {}",
            meeting_folder.display()
        );
    }

    Ok(meeting_folder)
}

/// Rename an existing meeting folder in place, returning the new path.
///
/// `new_folder_name` is sanitized to a single component, so the folder always
/// stays beside its siblings in the recordings directory. Unlike creation this
/// does not silently uniquify: an explicit rename onto an existing folder is
/// reported as an error so the caller can tell the user.
pub fn rename_meeting_folder(current: &Path, new_folder_name: &str) -> Result<PathBuf> {
    if !current.is_dir() {
        return Err(anyhow!(
            "Recording folder no longer exists: {}",
            current.display()
        ));
    }

    let sanitized = sanitize_component(new_folder_name);
    let parent = current
        .parent()
        .ok_or_else(|| anyhow!("Recording folder has no parent directory"))?;
    let target = parent.join(&sanitized);

    // Renaming to the identical path is a no-op, not an error. Compare the raw
    // names too so a case-only rename ("demo" -> "Demo") still reaches fs::rename
    // on case-insensitive filesystems.
    let unchanged = current
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == sanitized);
    if unchanged {
        return Ok(current.to_path_buf());
    }

    if target.exists() {
        return Err(anyhow!(
            "A folder named '{}' already exists in the recordings directory",
            sanitized
        ));
    }

    std::fs::rename(current, &target).map_err(|e| {
        anyhow!(
            "Failed to rename '{}' to '{}': {}",
            current.display(),
            sanitized,
            e
        )
    })?;

    log::info!(
        "Renamed meeting folder: {} -> {}",
        current.display(),
        target.display()
    );

    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_path_separators() {
        assert_eq!(sanitize_component("a/b\\c"), "a_b_c");
        assert_eq!(sanitize_component("Q1: Review"), "Q1_ Review");
    }

    #[test]
    fn sanitize_rejects_traversal_components() {
        assert_eq!(sanitize_component(".."), FALLBACK_NAME);
        assert_eq!(sanitize_component("."), FALLBACK_NAME);
        assert_eq!(sanitize_component("   "), FALLBACK_NAME);
        assert_eq!(sanitize_component(""), FALLBACK_NAME);
        // Separators are replaced before the traversal check, so this cannot escape.
        assert_eq!(sanitize_component("../../etc"), ".._.._etc");
    }

    #[test]
    fn sanitize_strips_trailing_dots_and_spaces() {
        assert_eq!(sanitize_component("Standup..."), "Standup");
        assert_eq!(sanitize_component("  Standup  "), "Standup");
    }

    #[test]
    fn build_folder_name_applies_prefix_verbatim() {
        assert_eq!(
            build_folder_name(Some("Work_"), "Standup", "2026-07-28_10-30"),
            "Work_Standup_2026-07-28_10-30"
        );
        assert_eq!(
            build_folder_name(Some("Work"), "Standup", "2026-07-28_10-30"),
            "WorkStandup_2026-07-28_10-30"
        );
        assert_eq!(
            build_folder_name(None, "Standup", "2026-07-28_10-30"),
            "Standup_2026-07-28_10-30"
        );
    }

    #[test]
    fn build_folder_name_ignores_unusable_prefix() {
        // A prefix of only whitespace or dots must not inject the fallback name.
        assert_eq!(
            build_folder_name(Some("   "), "Standup", "2026-07-28_10-30"),
            "Standup_2026-07-28_10-30"
        );
        assert_eq!(
            build_folder_name(Some(""), "Standup", "2026-07-28_10-30"),
            "Standup_2026-07-28_10-30"
        );
    }

    #[test]
    fn build_folder_name_sanitizes_prefix_separators() {
        assert_eq!(
            build_folder_name(Some("a/b-"), "Standup", "2026-07-28_10-30"),
            "a_b-Standup_2026-07-28_10-30"
        );
    }

    #[test]
    fn create_meeting_folder_uniquifies_collisions() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();

        let first = create_meeting_folder(base, "Standup", None, false).unwrap();
        let second = create_meeting_folder(base, "Standup", None, false).unwrap();

        assert_ne!(first, second, "same-minute meetings must not share a folder");
        assert!(first.is_dir() && second.is_dir());
        assert!(second
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("_2"));
    }

    #[test]
    fn create_meeting_folder_creates_checkpoints_only_when_requested() {
        let temp = tempfile::tempdir().unwrap();

        let with = create_meeting_folder(temp.path(), "With", None, true).unwrap();
        assert!(with.join(".checkpoints").is_dir());

        let without = create_meeting_folder(temp.path(), "Without", None, false).unwrap();
        assert!(!without.join(".checkpoints").exists());
    }

    #[test]
    fn create_meeting_folder_applies_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let folder = create_meeting_folder(temp.path(), "Standup", Some("Work_"), false).unwrap();

        assert!(folder
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Work_Standup_"));
    }

    #[test]
    fn rename_moves_folder_and_keeps_contents() {
        let temp = tempfile::tempdir().unwrap();
        let folder = create_meeting_folder(temp.path(), "Standup", None, false).unwrap();
        std::fs::write(folder.join("audio.mp4"), b"audio").unwrap();

        let renamed = rename_meeting_folder(&folder, "Weekly Sync").unwrap();

        assert_eq!(renamed, temp.path().join("Weekly Sync"));
        assert!(!folder.exists());
        assert_eq!(std::fs::read(renamed.join("audio.mp4")).unwrap(), b"audio");
    }

    #[test]
    fn rename_cannot_escape_the_recordings_directory() {
        let temp = tempfile::tempdir().unwrap();
        let folder = create_meeting_folder(temp.path(), "Standup", None, false).unwrap();

        let renamed = rename_meeting_folder(&folder, "../escaped").unwrap();

        assert_eq!(renamed.parent().unwrap(), temp.path());
        assert_eq!(renamed.file_name().unwrap(), ".._escaped");
    }

    #[test]
    fn rename_rejects_existing_target() {
        let temp = tempfile::tempdir().unwrap();
        let a = create_meeting_folder(temp.path(), "A", None, false).unwrap();
        let b = create_meeting_folder(temp.path(), "B", None, false).unwrap();
        let b_name = b.file_name().unwrap().to_string_lossy().to_string();

        let err = rename_meeting_folder(&a, &b_name).unwrap_err();

        assert!(err.to_string().contains("already exists"));
        assert!(a.is_dir(), "source folder must survive a failed rename");
    }

    #[test]
    fn rename_to_same_name_is_a_noop() {
        let temp = tempfile::tempdir().unwrap();
        let folder = create_meeting_folder(temp.path(), "Standup", None, false).unwrap();
        let name = folder.file_name().unwrap().to_string_lossy().to_string();

        let renamed = rename_meeting_folder(&folder, &name).unwrap();

        assert_eq!(renamed, folder);
        assert!(folder.is_dir());
    }

    /// Full chain a user goes through: a prefixed folder is created, a recording
    /// is written into it, the user renames the folder, and the audio must still
    /// resolve from the new path (which is what playback and retranscription use).
    #[test]
    fn recording_stays_resolvable_across_a_rename() {
        use crate::audio::retranscription::find_audio_file;

        for extension in ["mp4", "mov"] {
            let temp = tempfile::tempdir().unwrap();
            let config = MeetingFolderConfig {
                base_folder: temp.path().to_path_buf(),
                prefix: Some("Work_".to_string()),
            };

            let folder = config.create("Team Standup", true).unwrap();
            assert!(folder
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("Work_Team Standup_"));

            std::fs::write(folder.join(format!("audio.{}", extension)), b"audio").unwrap();
            std::fs::write(folder.join("transcripts.json"), b"[]").unwrap();
            assert_eq!(
                find_audio_file(&folder).unwrap(),
                folder.join(format!("audio.{}", extension))
            );

            let renamed = rename_meeting_folder(&folder, "Q3 Kickoff").unwrap();

            assert_eq!(
                find_audio_file(&renamed).unwrap(),
                renamed.join(format!("audio.{}", extension)),
                "audio.{} must still resolve after the folder is renamed",
                extension
            );
            assert!(renamed.join("transcripts.json").is_file());
            assert!(renamed.join(".checkpoints").is_dir());
        }
    }

    #[test]
    fn rename_reports_missing_folder() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("gone");

        let err = rename_meeting_folder(&missing, "New Name").unwrap_err();

        assert!(err.to_string().contains("no longer exists"));
    }
}
