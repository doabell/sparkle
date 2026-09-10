//! User-initiated updates. Merely starting Sparkle or opening Settings never
//! contacts the release server, downloads an update, or applies a cached one.
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

pub const APP_ID: &str = "com.doabell.sparkle";
const REPOSITORY: &str = "https://github.com/doabell/sparkle";
const UNAVAILABLE: &str =
    "Install Sparkle using the Windows setup from GitHub Releases to use in-app updates.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Ready,
    Installing,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
pub struct Release {
    pub version: String,
    pub size: u64,
    pub notes: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct UpdateStatus {
    pub revision: u64,
    pub current_version: String,
    pub phase: Phase,
    pub release: Option<Release>,
    pub download_percent: Option<u8>,
    pub error: Option<String>,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self {
            revision: 0,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            phase: Phase::Idle,
            release: None,
            download_percent: None,
            error: None,
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use velopack::{sources::GithubSource, UpdateCheck, UpdateInfo, UpdateManager};

    /// Kept at the package boundary so tests exercise real transitions without
    /// contacting GitHub, installing software, or terminating the test process.
    pub(super) trait Backend: Send + Sync {
        fn pending(&self) -> Option<UpdateInfo>;
        fn check(&self) -> Result<Option<UpdateInfo>, String>;
        fn download(
            &self,
            update: &UpdateInfo,
            progress: &(dyn Fn(u8) + Sync),
        ) -> Result<(), String>;
        fn prepare_restart(&self, update: &UpdateInfo) -> Result<(), String>;
    }

    struct NativeBackend(UpdateManager);

    impl Backend for NativeBackend {
        fn pending(&self) -> Option<UpdateInfo> {
            self.0.get_update_pending_restart().map(|asset| UpdateInfo {
                TargetFullRelease: asset,
                ..Default::default()
            })
        }

        fn check(&self) -> Result<Option<UpdateInfo>, String> {
            match self.0.check_for_updates().map_err(|e| e.to_string())? {
                UpdateCheck::UpdateAvailable(update) => Ok(Some(*update)),
                UpdateCheck::NoUpdateAvailable => Ok(None),
                UpdateCheck::RemoteIsEmpty => {
                    Err("No published update packages are available yet. Try again later.".into())
                }
            }
        }

        fn download(
            &self,
            update: &UpdateInfo,
            progress: &(dyn Fn(u8) + Sync),
        ) -> Result<(), String> {
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::scope(|scope| {
                scope.spawn(move || {
                    for percent in receiver {
                        progress(i16::clamp(percent, 0, 100) as u8);
                    }
                });
                // Velopack verifies the downloaded package against the feed's
                // checksum before moving it out of its partial-download file.
                self.0
                    .download_updates(update, Some(sender))
                    .map_err(|e| e.to_string())
            })
        }

        fn prepare_restart(&self, update: &UpdateInfo) -> Result<(), String> {
            // This starts Update.exe but lets Tauri perform its normal shutdown
            // (flush listening history, close audio/media and the database).
            self.0
                .wait_exit_then_apply_updates(update, false, true, Vec::<String>::new())
                .map_err(|e| e.to_string())
        }
    }

    #[derive(Default)]
    struct Session {
        status: UpdateStatus,
        backend: Option<Box<dyn Backend>>,
        update: Option<UpdateInfo>,
        initialized: bool,
    }

    impl Session {
        fn attach(&mut self, backend: Box<dyn Backend>) {
            if let Some(pending) = backend
                .pending()
                .filter(|update| valid_release(update).is_ok())
            {
                self.status.release = Some(release_summary(&pending));
                self.status.phase = Phase::Ready;
                self.update = Some(pending);
            }
            self.backend = Some(backend);
            self.initialized = true;
        }
    }

    #[derive(Default)]
    pub struct UpdateState(Arc<Mutex<Session>>);

    impl Clone for UpdateState {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl UpdateState {
        fn lock(&self) -> std::sync::MutexGuard<'_, Session> {
            self.0.lock().unwrap_or_else(|e| e.into_inner())
        }

        pub fn status(&self) -> UpdateStatus {
            let mut session = self.lock();
            if !session.initialized {
                session.initialized = true;
                // Local manifest discovery only. Development and legacy MSI
                // installations remain usable, with a link to the new setup.
                let manager = if cfg!(all(not(debug_assertions), target_arch = "x86_64")) {
                    UpdateManager::new(GithubSource::new(REPOSITORY, None, false), None, None).ok()
                } else {
                    None
                };
                match manager
                    .filter(|manager| manager.get_app_id() == APP_ID && !manager.get_is_portable())
                {
                    Some(manager) => {
                        session.attach(Box::new(NativeBackend(manager)));
                    }
                    None => {
                        session.status.phase = Phase::Unavailable;
                        session.status.error = Some(UNAVAILABLE.into());
                    }
                }
            }
            session.status.clone()
        }

        /// Reserve an operation before releasing the lock for blocking I/O.
        /// Status reads and download progress remain responsive in the meantime.
        fn begin(&self, phase: Phase) -> Result<(Phase, Box<dyn Backend>), String> {
            self.status();
            let mut session = self.lock();
            let previous = session.status.phase;
            let allowed = match phase {
                Phase::Checking => {
                    matches!(previous, Phase::Idle | Phase::UpToDate | Phase::Available)
                }
                Phase::Downloading => previous == Phase::Available,
                Phase::Installing => previous == Phase::Ready,
                _ => false,
            };
            if !allowed {
                return Err(match previous {
                    Phase::Unavailable => UNAVAILABLE.into(),
                    Phase::Checking | Phase::Downloading | Phase::Installing => {
                        "An update operation is already in progress.".into()
                    }
                    _ => "Check for an update, download it, then choose Restart to install.".into(),
                });
            }
            let backend = session.backend.take().ok_or("Updater is unavailable.")?;
            session.status.phase = phase;
            session.status.error = None;
            session.status.download_percent = (phase == Phase::Downloading).then_some(0);
            session.status.revision += 1;
            Ok((previous, backend))
        }

        pub fn check(&self, notify: &dyn Fn(UpdateStatus)) -> Result<UpdateStatus, String> {
            let (previous, backend) = self.begin(Phase::Checking)?;
            notify(self.status());
            let result = backend.check().and_then(|update| {
                if let Some(ref update) = update {
                    valid_release(update)?;
                }
                Ok(update)
            });
            let mut session = self.lock();
            session.backend = Some(backend);
            match result {
                Ok(update) => {
                    session.status.release = update.as_ref().map(release_summary);
                    session.status.phase = if update.is_some() {
                        Phase::Available
                    } else {
                        Phase::UpToDate
                    };
                    session.update = update;
                }
                Err(error) => {
                    session.status.phase = if previous == Phase::UpToDate {
                        Phase::Idle
                    } else {
                        previous
                    };
                    session.status.error = Some(error);
                }
            }
            session.status.revision += 1;
            Ok(session.status.clone())
        }

        pub fn download(
            &self,
            notify: &(dyn Fn(UpdateStatus) + Sync),
        ) -> Result<UpdateStatus, String> {
            let (previous, backend) = self.begin(Phase::Downloading)?;
            notify(self.status());
            let update = self.lock().update.clone();
            let result = match update {
                Some(update) => backend.download(&update, &|percent| {
                    let snapshot = {
                        let mut session = self.lock();
                        session.status.download_percent = Some(percent);
                        session.status.revision += 1;
                        session.status.clone()
                    };
                    notify(snapshot);
                }),
                None => Err("No update has been selected.".into()),
            };
            let mut session = self.lock();
            session.backend = Some(backend);
            match result {
                Ok(()) => {
                    session.status.phase = Phase::Ready;
                    session.status.download_percent = Some(100);
                }
                Err(error) => {
                    session.status.phase = previous;
                    session.status.error = Some(error);
                    session.status.download_percent = None;
                }
            }
            session.status.revision += 1;
            Ok(session.status.clone())
        }

        pub fn install(&self) -> Result<UpdateStatus, String> {
            let (previous, backend) = self.begin(Phase::Installing)?;
            let update = self.lock().update.clone();
            let result = match update {
                Some(update) => backend.prepare_restart(&update),
                None => Err("No downloaded update is ready.".into()),
            };
            let mut session = self.lock();
            session.backend = Some(backend);
            if let Err(error) = result {
                session.status.phase = previous;
                session.status.error = Some(error);
            }
            session.status.revision += 1;
            Ok(session.status.clone())
        }
    }

    fn release_summary(update: &UpdateInfo) -> Release {
        let asset = &update.TargetFullRelease;
        Release {
            version: asset.Version.clone(),
            size: asset.Size,
            notes: asset.NotesMarkdown.clone(),
        }
    }

    fn valid_release(update: &UpdateInfo) -> Result<(), String> {
        let asset = &update.TargetFullRelease;
        if asset.PackageId != APP_ID
            || update.IsDowngrade
            || !asset.Type.eq_ignore_ascii_case("Full")
            || asset.FileName.is_empty()
            || asset.FileName.contains(['/', '\\', ':'])
            || !asset.FileName.ends_with(".nupkg")
        {
            return Err("The release does not contain a compatible Sparkle update.".into());
        }
        Ok(())
    }

    #[cfg(test)]
    include!("tests/updates.rs");
}

#[cfg(windows)]
pub use platform::UpdateState;

#[cfg(not(windows))]
#[derive(Clone, Default)]
pub struct UpdateState;

#[cfg(not(windows))]
impl UpdateState {
    fn status(&self) -> UpdateStatus {
        UpdateStatus {
            phase: Phase::Unavailable,
            error: Some(UNAVAILABLE.into()),
            ..Default::default()
        }
    }
}

#[tauri::command]
pub async fn get_update_status(state: State<'_, UpdateState>) -> Result<UpdateStatus, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.status())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<UpdateStatus, String> {
    #[cfg(windows)]
    {
        let state = state.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let status = state.check(&|status| {
                let _ = app.emit("update-status", status);
            })?;
            let _ = app.emit("update-status", &status);
            Ok(status)
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = (app, state);
        Err(UNAVAILABLE.into())
    }
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<UpdateStatus, String> {
    #[cfg(windows)]
    {
        let state = state.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let status = state.download(&|status| {
                let _ = app.emit("update-status", status);
            })?;
            let _ = app.emit("update-status", &status);
            Ok(status)
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = (app, state);
        Err(UNAVAILABLE.into())
    }
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<UpdateStatus, String> {
    #[cfg(windows)]
    {
        let state = state.inner().clone();
        let status = tauri::async_runtime::spawn_blocking(move || state.install())
            .await
            .map_err(|e| e.to_string())??;
        let _ = app.emit("update-status", &status);
        if status.phase == Phase::Installing {
            app.exit(0);
        }
        Ok(status)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, state);
        Err(UNAVAILABLE.into())
    }
}
