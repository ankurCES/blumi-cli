//! Resolved filesystem locations. Not loaded from config files — computed at
//! startup from the home directory and the working directory.

use std::path::{Path, PathBuf};

/// All the paths blumi reads/writes at runtime.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paths {
    /// The blumi data home, default `~/.blumi`.
    pub home: PathBuf,
    /// SQLite database, `<home>/blumi.db`.
    pub db: PathBuf,
    /// User skills directory, `<home>/skills`.
    pub skills: PathBuf,
    /// Memory directory holding MEMORY.md and USER.md, `<home>/memory`.
    pub memory_dir: PathBuf,
    /// Session JSONL exports, `<home>/sessions`.
    pub sessions: PathBuf,
    /// Cache for bundled/downloaded models (e.g. the local embedder),
    /// `<home>/models`.
    pub models_dir: PathBuf,
    /// Code knowledge base, `<home>/knowledge.db` (wipeable independently).
    pub knowledge_db: PathBuf,
    /// The project / working directory the agent operates in.
    pub working_dir: PathBuf,
}

impl Paths {
    /// Resolve all paths. `home_override` (e.g. from `BLUMI_HOME`) wins over the
    /// default `~/.blumi`; if no home can be found, falls back to `./.blumi`.
    pub fn resolve(home_override: Option<PathBuf>, working_dir: impl AsRef<Path>) -> Self {
        let home = home_override
            .or_else(|| dirs::home_dir().map(|h| h.join(".blumi")))
            .unwrap_or_else(|| working_dir.as_ref().join(".blumi"));

        Paths {
            db: home.join("blumi.db"),
            skills: home.join("skills"),
            memory_dir: home.join("memory"),
            sessions: home.join("sessions"),
            models_dir: home.join("models"),
            knowledge_db: home.join("knowledge.db"),
            working_dir: working_dir.as_ref().to_path_buf(),
            home,
        }
    }

    pub fn memory_md(&self) -> PathBuf {
        self.memory_dir.join("MEMORY.md")
    }

    pub fn user_md(&self) -> PathBuf {
        self.memory_dir.join("USER.md")
    }

    /// The user settings file, `<home>/settings.json`.
    pub fn settings_json(&self) -> PathBuf {
        self.home.join("settings.json")
    }

    /// Always-on discovery reports, `<home>/reports`.
    pub fn reports_dir(&self) -> PathBuf {
        self.home.join("reports")
    }

    /// Web Push state (VAPID keypair + browser subscriptions), `<home>/push.json`.
    pub fn push_store(&self) -> PathBuf {
        self.home.join("push.json")
    }

    /// FCM service account (the gateway's sender key for blugo phone push),
    /// `<home>/fcm-service-account.json`. Drop the Firebase admin-SDK JSON here
    /// and FCM push turns on automatically — no settings required.
    pub fn fcm_service_account(&self) -> PathBuf {
        self.home.join("fcm-service-account.json")
    }

    /// Registered blugo device FCM tokens, `<home>/fcm.json`.
    pub fn fcm_store(&self) -> PathBuf {
        self.home.join("fcm.json")
    }

    /// Create the home, skills, memory, sessions, and models directories.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            &self.home,
            &self.skills,
            &self.memory_dir,
            &self.sessions,
            &self.models_dir,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_with_override() {
        let p = Paths::resolve(Some(PathBuf::from("/data/blumi")), "/work/proj");
        assert_eq!(p.home, PathBuf::from("/data/blumi"));
        assert_eq!(p.db, PathBuf::from("/data/blumi/blumi.db"));
        assert_eq!(p.memory_md(), PathBuf::from("/data/blumi/memory/MEMORY.md"));
        assert_eq!(p.working_dir, PathBuf::from("/work/proj"));
        assert_eq!(
            p.fcm_service_account(),
            PathBuf::from("/data/blumi/fcm-service-account.json")
        );
        assert_eq!(p.fcm_store(), PathBuf::from("/data/blumi/fcm.json"));
    }
}
