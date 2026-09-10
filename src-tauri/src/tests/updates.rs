mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use velopack::VelopackAsset;

    #[derive(Default)]
    struct Calls {
        checks: AtomicUsize,
        downloads: AtomicUsize,
        installs: AtomicUsize,
    }

    struct FakeBackend {
        calls: Arc<Calls>,
        fail_download_once: std::sync::atomic::AtomicBool,
        fail_install: bool,
        offer: Option<UpdateInfo>,
        check_error: bool,
    }

    impl Backend for FakeBackend {
        fn pending(&self) -> Option<UpdateInfo> {
            None
        }

        fn check(&self) -> Result<Option<UpdateInfo>, String> {
            self.calls.checks.fetch_add(1, Ordering::Relaxed);
            if self.check_error {
                Err("GitHub is unavailable.".into())
            } else {
                Ok(self.offer.clone())
            }
        }

        fn download(&self, _: &UpdateInfo, progress: &(dyn Fn(u8) + Sync)) -> Result<(), String> {
            self.calls.downloads.fetch_add(1, Ordering::Relaxed);
            progress(35);
            if self.fail_download_once.swap(false, Ordering::Relaxed) {
                Err("Download interrupted.".into())
            } else {
                progress(100);
                Ok(())
            }
        }

        fn prepare_restart(&self, _: &UpdateInfo) -> Result<(), String> {
            self.calls.installs.fetch_add(1, Ordering::Relaxed);
            if self.fail_install {
                Err("Could not start the updater.".into())
            } else {
                Ok(())
            }
        }
    }

    fn offer() -> UpdateInfo {
        UpdateInfo {
            TargetFullRelease: VelopackAsset {
                PackageId: APP_ID.into(),
                Version: "0.5.0".into(),
                Type: "Full".into(),
                FileName: "com.doabell.sparkle-0.5.0-full.nupkg".into(),
                Size: 42,
                NotesMarkdown: "Playback improvements.".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn setup(
        fail_download: bool,
        fail_install: bool,
        check_error: bool,
    ) -> (UpdateState, Arc<Calls>) {
        let calls = Arc::new(Calls::default());
        let state = UpdateState(Arc::new(Mutex::new(Session {
            backend: Some(Box::new(FakeBackend {
                calls: calls.clone(),
                fail_download_once: fail_download.into(),
                fail_install,
                offer: Some(offer()),
                check_error,
            })),
            initialized: true,
            ..Default::default()
        })));
        (state, calls)
    }

    #[test]
    fn every_network_and_install_step_requires_its_own_action() {
        let (state, calls) = setup(false, false, false);
        assert_eq!(state.status().phase, Phase::Idle);
        assert_eq!(calls.checks.load(Ordering::Relaxed), 0);
        assert!(state.download(&|_| {}).is_err());
        assert!(state.install().is_err());
        let checked = state.check(&|_| {}).unwrap();
        assert_eq!(checked.phase, Phase::Available);
        assert_eq!(checked.release.unwrap().version, "0.5.0");
        assert_eq!(calls.downloads.load(Ordering::Relaxed), 0);
        assert_eq!(calls.installs.load(Ordering::Relaxed), 0);
        assert!(state.install().is_err());

        let downloaded = state.download(&|_| {}).unwrap();
        assert_eq!(downloaded.phase, Phase::Ready);
        assert_eq!(downloaded.download_percent, Some(100));
        assert_eq!(calls.installs.load(Ordering::Relaxed), 0);
        assert_eq!(state.status().phase, Phase::Ready);
        assert!(state.check(&|_| {}).is_err());
        assert_eq!(state.install().unwrap().phase, Phase::Installing);
        assert_eq!(calls.installs.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn interrupted_download_can_be_retried_without_installing() {
        let (state, calls) = setup(true, false, false);
        state.check(&|_| {}).unwrap();
        let failed = state.download(&|_| {}).unwrap();
        assert_eq!(failed.phase, Phase::Available);
        assert_eq!(failed.error.as_deref(), Some("Download interrupted."));
        assert_eq!(failed.download_percent, None);
        assert!(state.install().is_err());
        let ready = state.download(&|_| {}).unwrap();
        assert_eq!(ready.phase, Phase::Ready);
        assert!(ready.error.is_none());
        assert_eq!(calls.downloads.load(Ordering::Relaxed), 2);
        assert_eq!(calls.installs.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn failed_check_does_not_claim_the_app_is_up_to_date() {
        let (state, _) = setup(false, false, true);
        let failed = state.check(&|_| {}).unwrap();
        assert_eq!(failed.phase, Phase::Idle);
        assert!(failed.error.is_some());
        assert!(state.download(&|_| {}).is_err());
        state.lock().status.phase = Phase::UpToDate;
        let failed_recheck = state.check(&|_| {}).unwrap();
        assert_eq!(failed_recheck.phase, Phase::Idle);
        assert!(failed_recheck.error.is_some());
    }

    #[test]
    fn failed_restart_retains_download_for_retry() {
        let (state, _) = setup(false, true, false);
        state.check(&|_| {}).unwrap();
        state.download(&|_| {}).unwrap();
        let failed = state.install().unwrap();
        assert_eq!(failed.phase, Phase::Ready);
        assert_eq!(failed.release.unwrap().version, "0.5.0");
        assert!(failed.error.is_some());
    }

    #[test]
    fn progress_does_not_lock_status_and_blocks_concurrent_mutations() {
        let (state, _) = setup(false, false, false);
        state.check(&|_| {}).unwrap();
        let revisions = Mutex::new(Vec::new());
        state
            .download(&|snapshot| {
                assert_eq!(state.status().phase, Phase::Downloading);
                assert!(state.check(&|_| {}).is_err());
                assert!(state.download(&|_| {}).is_err());
                assert!(state.install().is_err());
                revisions.lock().unwrap().push(snapshot.revision);
            })
            .unwrap();
        let revisions = revisions.lock().unwrap();
        assert!(revisions.windows(2).all(|pair| pair[1] > pair[0]));
        assert!(state.status().revision > *revisions.last().unwrap());
    }

    #[test]
    fn rejects_other_apps_downgrades_and_paths_from_release_metadata() {
        for filename in [
            "../payload.nupkg",
            r"..\payload.nupkg",
            "C:payload.nupkg",
            "app.exe",
            "",
        ] {
            let mut update = offer();
            update.TargetFullRelease.FileName = filename.into();
            assert!(valid_release(&update).is_err());
        }
        let mut update = offer();
        update.TargetFullRelease.PackageId = "other-app".into();
        assert!(valid_release(&update).is_err());
        let mut update = offer();
        update.IsDowngrade = true;
        assert!(valid_release(&update).is_err());
        assert!(valid_release(&offer()).is_ok());
    }

    #[test]
    fn native_feed_verifies_download_and_recovers_pending_package_offline() {
        use crate::test_support::TestDir;
        use sha2::{Digest, Sha256};
        use std::fs;
        use velopack::{locator::VelopackLocatorConfig, sources::FileSource, VelopackAssetFeed};

        let dir = TestDir::new();
        let feed = dir.join("feed");
        let packages = dir.join("packages");
        fs::create_dir(&feed).unwrap();
        fs::create_dir(&packages).unwrap();
        fs::write(dir.join("Update.exe"), []).unwrap();
        fs::write(dir.join("sq.version"), r#"<package><metadata><id>com.doabell.sparkle</id><version>0.4.0</version><channel>win-x64</channel><mainExe>sparkle.exe</mainExe></metadata></package>"#).unwrap();
        let locator = VelopackLocatorConfig {
            RootAppDir: dir.path().into(),
            UpdateExePath: dir.join("Update.exe"),
            PackagesDir: packages.clone(),
            ManifestPath: dir.join("sq.version"),
            CurrentBinaryDir: dir.path().into(),
            IsPortable: false,
        };
        let manager =
            || UpdateManager::new(FileSource::new(&feed), None, Some(locator.clone())).unwrap();
        let state = UpdateState::default();
        state.lock().attach(Box::new(NativeBackend(manager())));
        // There is no feed yet: reading local status must succeed without it.
        assert_eq!(state.status().phase, Phase::Idle);

        let bytes = include_bytes!("fixtures/update-0.5.0.nupkg");
        let mut asset = offer().TargetFullRelease;
        asset.Size = bytes.len() as u64;
        asset.SHA256 = format!("{:X}", Sha256::digest(bytes));
        fs::write(
            feed.join("releases.win-x64.json"),
            serde_json::to_vec(&VelopackAssetFeed {
                Assets: vec![asset.clone()],
            })
            .unwrap(),
        )
        .unwrap();
        let checked = state.check(&|_| {}).unwrap();
        assert_eq!(checked.phase, Phase::Available);
        assert!(!packages.join(&asset.FileName).exists());

        let mut corrupt = bytes.to_vec();
        corrupt[0] ^= 1;
        fs::write(feed.join(&asset.FileName), corrupt).unwrap();
        let failed = state.download(&|_| {}).unwrap();
        assert_eq!(failed.phase, Phase::Available);
        assert!(failed.error.unwrap().contains("Checksum"));
        assert!(!packages.join(&asset.FileName).exists());
        assert!(state.install().is_err());

        fs::write(feed.join(&asset.FileName), bytes).unwrap();
        assert_eq!(state.download(&|_| {}).unwrap().phase, Phase::Ready);
        assert_eq!(fs::read(packages.join(&asset.FileName)).unwrap(), bytes);
        fs::remove_file(feed.join("releases.win-x64.json")).unwrap();
        let reopened = UpdateState::default();
        reopened.lock().attach(Box::new(NativeBackend(manager())));
        let restored = reopened.status();
        assert_eq!(restored.phase, Phase::Ready);
        assert_eq!(restored.release.unwrap().version, "0.5.0");
        // A missing/invalid updater is reported without discarding the download.
        assert_eq!(reopened.install().unwrap().phase, Phase::Ready);
        assert!(reopened.status().error.is_some());
    }
}
