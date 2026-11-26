//! Machine-local layered overrides (`config.local.toml`) when sharing `config.toml` across PCs.

use super::TomoyoConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Machine-specific fields kept strictly local in `config.local.toml` when sync directory is set.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalConfigOverride {
    #[serde(default)]
    pub apps_extra_dirs: Option<Vec<String>>,
    #[serde(default)]
    pub capture_save_dir: Option<String>,
    #[serde(default)]
    pub hotkey_excluded_apps: Option<Vec<String>>,
    #[serde(default)]
    pub window_custom_x: Option<i32>,
    #[serde(default)]
    pub window_custom_y: Option<i32>,
    #[serde(default)]
    pub update_last_checked_unix: Option<u64>,
}

pub fn local_override_path() -> PathBuf {
    crate::data_dir().join("config.local.toml")
}

impl LocalConfigOverride {
    pub fn from_config(cfg: &TomoyoConfig) -> Self {
        Self {
            apps_extra_dirs: Some(cfg.apps.extra_dirs.clone()),
            capture_save_dir: Some(cfg.capture.save_dir.clone()),
            hotkey_excluded_apps: Some(cfg.hotkey_excluded_apps.clone()),
            window_custom_x: cfg.window.custom_x,
            window_custom_y: cfg.window.custom_y,
            update_last_checked_unix: (cfg.update.last_checked_unix > 0)
                .then_some(cfg.update.last_checked_unix),
        }
    }

    pub fn apply_to(&self, cfg: &mut TomoyoConfig) {
        if let Some(dirs) = &self.apps_extra_dirs {
            cfg.apps.extra_dirs = dirs.clone();
        }
        if let Some(save_dir) = &self.capture_save_dir {
            cfg.capture.save_dir = save_dir.clone();
        }
        if let Some(apps) = &self.hotkey_excluded_apps {
            cfg.hotkey_excluded_apps = apps.clone();
        }
        if self.window_custom_x.is_some() || self.window_custom_y.is_some() {
            cfg.window.custom_x = self.window_custom_x;
            cfg.window.custom_y = self.window_custom_y;
        }
        if let Some(ts) = self.update_last_checked_unix {
            cfg.update.last_checked_unix = ts;
        }
    }
}

/// Applies `config.local.toml` overrides onto `cfg` if present and valid.
pub fn apply_local_override_from(cfg: &mut TomoyoConfig, local_path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(local_path) else {
        return false;
    };
    match toml::from_str::<LocalConfigOverride>(&raw) {
        Ok(ov) => {
            ov.apply_to(cfg);
            true
        }
        Err(e) => {
            tracing::warn!("Invalid config.local.toml at {}: {e}", local_path.display());
            false
        }
    }
}

/// Atomically writes machine-local overrides to `local_path`.
pub fn save_local_override_to(cfg: &TomoyoConfig, local_path: &Path) -> std::io::Result<()> {
    let ov = LocalConfigOverride::from_config(cfg);
    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = toml::to_string_pretty(&ov).unwrap_or_default();
    let header = "# Tomoyo Machine-Local Overrides (kept off synced folders)\n\
                  # Automatically layered on top of synced config.toml.\n\n";
    let tmp = local_path.with_extension("toml.tmp");
    std::fs::write(&tmp, format!("{header}{body}"))?;
    std::fs::rename(&tmp, local_path)?;
    Ok(())
}

/// Sanitizes `cfg` before writing to a synced shared `config.toml` so machine-bound window
/// coordinates, update timestamps, and existing shared paths are never clobbered across PCs.
pub fn prepare_shared_config_for_sync(
    cfg: &TomoyoConfig,
    existing_shared: Option<&TomoyoConfig>,
) -> TomoyoConfig {
    let mut shared = cfg.clone();
    shared.window.custom_x = None;
    shared.window.custom_y = None;
    shared.update.last_checked_unix = 0;
    if let Some(prev) = existing_shared {
        shared.apps.extra_dirs = prev.apps.extra_dirs.clone();
        shared.capture.save_dir = prev.capture.save_dir.clone();
        shared.hotkey_excluded_apps = prev.hotkey_excluded_apps.clone();
    }
    shared
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tomoyo_local_ov_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sync_mode_isolates_machine_paths_and_window_coords_across_two_pcs() {
        let dir = temp_dir("isolate");
        let shared_path = dir.join("shared_config.toml");
        let work_local = dir.join("work_local.toml");
        let home_local = dir.join("home_local.toml");

        let mut base = TomoyoConfig::default();
        base.apps.extra_dirs = vec![r"D:\HomeTools".into()];
        base.capture.save_dir = r"D:\HomeShots".into();
        base.hotkey_excluded_apps = vec!["game.exe".into()];
        base.save_to_path(&shared_path).unwrap();

        let mut work = base.clone();
        work.theme = "midnight".into();
        work.apps.extra_dirs = vec![r"E:\WorkTools".into()];
        work.capture.save_dir = r"E:\WorkShots".into();
        work.hotkey_excluded_apps = vec!["rustdesk.exe".into()];
        work.window.custom_x = Some(420);
        work.window.custom_y = Some(180);
        work.update.last_checked_unix = 1795650000;
        save_local_override_to(&work, &work_local).unwrap();

        let shared_for_write = prepare_shared_config_for_sync(&work, Some(&base));
        shared_for_write.save_to_path(&shared_path).unwrap();

        let mut reloaded_shared = TomoyoConfig::load_from_path(&shared_path);
        assert_eq!(reloaded_shared.theme, "midnight");
        assert_eq!(reloaded_shared.window.custom_x, None);
        assert_eq!(reloaded_shared.apps.extra_dirs, vec![r"D:\HomeTools".to_string()]);
        assert_eq!(reloaded_shared.capture.save_dir, r"D:\HomeShots");

        let mut reloaded_work = reloaded_shared.clone();
        assert!(apply_local_override_from(&mut reloaded_work, &work_local));
        assert_eq!(reloaded_work.apps.extra_dirs, vec![r"E:\WorkTools".to_string()]);
        assert_eq!(reloaded_work.capture.save_dir, r"E:\WorkShots");
        assert_eq!(reloaded_work.hotkey_excluded_apps, vec!["rustdesk.exe".to_string()]);
        assert_eq!(reloaded_work.window.custom_x, Some(420));

        assert!(!apply_local_override_from(&mut reloaded_shared, &home_local));
        assert_eq!(reloaded_shared.theme, "midnight");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_mode_shares_portable_theme_hotkeys_and_web_searches() {
        let dir = temp_dir("portable");
        let local_path = dir.join("config.local.toml");
        let mut cfg = TomoyoConfig {
            hotkey: "Ctrl+Space".into(),
            language: "en".into(),
            ..Default::default()
        };
        cfg.apps.extra_dirs = vec![r"C:\Bin".into()];
        save_local_override_to(&cfg, &local_path).unwrap();

        let shared = prepare_shared_config_for_sync(&cfg, None);
        assert_eq!(shared.hotkey, "Ctrl+Space");
        assert_eq!(shared.language, "en");
        let mut on_other_pc = shared;
        on_other_pc.apps.extra_dirs = vec![r"/opt/bin".into()];
        assert!(apply_local_override_from(&mut on_other_pc, &local_path));
        assert_eq!(on_other_pc.hotkey, "Ctrl+Space");
        assert_eq!(on_other_pc.apps.extra_dirs, vec![r"C:\Bin".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_local_override_is_harmless_noop() {
        let mut cfg = TomoyoConfig::default();
        let orig_dir = cfg.capture.save_dir.clone();
        assert!(!apply_local_override_from(&mut cfg, Path::new("nonexistent_tomoyo_local.toml")));
        assert_eq!(cfg.capture.save_dir, orig_dir);
    }
}
