use super::*;

#[test]
fn progress_and_results_remain_available_between_ui_subscriptions() {
    let scan = Arc::new(LibraryScan::default());
    let revisions = Arc::new(Mutex::new(Vec::new()));
    let observed_scan = scan.clone();
    let observed_revisions = revisions.clone();
    let mut run = scan
        .begin(move |status| {
            // Event handling can safely request a fresh snapshot.
            assert_eq!(observed_scan.status().revision, status.revision);
            observed_revisions.lock().unwrap().push(status.revision);
        })
        .unwrap();
    assert!(scan.status().running);
    run.progress(ScanProgress {
        phase: "scanning".into(),
        current_path: Some("song.flac".into()),
        scanned: 3,
        total: 10,
        added: 1,
        updated: 2,
        removed: 0,
        errors: 0,
    });
    for _ in 0..2 {
        let snapshot = scan.status();
        assert!(snapshot.running);
        assert_eq!(snapshot.progress.unwrap().scanned, 3);
    }
    assert!(scan.begin(|_| {}).is_err());
    assert_eq!(scan.status().revision, 2);
    run.finish(&Ok(ScanResult {
        scanned: 10,
        added: 1,
        updated: 2,
        removed: 0,
        errors: 0,
    }));
    let finished = scan.status();
    assert!(!finished.running);
    assert!(finished.progress.is_none());
    assert!(finished.error.is_none());
    assert_eq!(finished.result.unwrap().scanned, 10);
    assert_eq!(*revisions.lock().unwrap(), vec![1, 2, 3]);
    let _next = scan.begin(|_| {}).unwrap();
    assert!(scan.status().result.is_none());
}

#[test]
fn fast_scans_coalesce_events_without_losing_snapshots_or_completion() {
    let scan = LibraryScan::default();
    let events = Arc::new(Mutex::new(Vec::new()));
    let collected = events.clone();
    let mut run = scan
        .begin(move |status| collected.lock().unwrap().push(status))
        .unwrap();
    let start = Instant::now();
    let progress = |scanned, phase: &str| ScanProgress {
        phase: phase.into(),
        current_path: None,
        scanned,
        total: 50,
        added: 0,
        updated: scanned,
        removed: 0,
        errors: 0,
    };
    for scanned in 0..50 {
        run.progress_at(
            progress(scanned, "scanning"),
            start + Duration::from_millis(scanned as u64 * 5),
        );
        assert_eq!(scan.status().progress.unwrap().scanned, scanned);
    }
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|status| status.progress.as_ref().map(|p| p.scanned))
            .collect::<Vec<_>>(),
        vec![0, 20, 40]
    );
    run.progress_at(progress(50, "cleaning"), start + Duration::from_millis(250));
    assert_eq!(
        events
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .progress
            .as_ref()
            .unwrap()
            .phase,
        "cleaning"
    );
    run.finish(&Ok(ScanResult {
        scanned: 50,
        added: 0,
        updated: 50,
        removed: 0,
        errors: 0,
    }));
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 6); // Start, three progress events, cleaning, completion.
    let last = events.last().unwrap();
    assert!(!last.running);
    assert_eq!(last.result.as_ref().unwrap().scanned, 50);
    assert_eq!(last.revision, scan.status().revision);
}

#[test]
fn failures_and_abandoned_workers_release_the_scan_guard() {
    let scan = LibraryScan::default();
    let run = scan.begin(|_| {}).unwrap();
    run.finish(&Err("folder unavailable".into()));
    assert!(!scan.status().running);
    assert_eq!(scan.status().error.as_deref(), Some("folder unavailable"));
    let run = scan.begin(|_| {}).unwrap();
    assert!(scan.status().error.is_none());
    drop(run);
    assert!(!scan.status().running);
    assert_eq!(
        scan.status().error.as_deref(),
        Some("Library scan stopped unexpectedly")
    );
    assert!(scan.begin(|_| {}).is_ok());
}

#[test]
fn simultaneous_startup_and_manual_requests_claim_exactly_one_scan() {
    let scan = Arc::new(LibraryScan::default());
    let ready = Arc::new(std::sync::Barrier::new(3));
    let release = Arc::new(std::sync::Barrier::new(3));
    let (tx, rx) = std::sync::mpsc::channel();
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let (scan, ready, release, tx) =
                (scan.clone(), ready.clone(), release.clone(), tx.clone());
            std::thread::spawn(move || {
                ready.wait();
                let run = scan.begin(|_| {});
                tx.send(run.is_ok()).unwrap();
                release.wait();
                drop(run);
            })
        })
        .collect();
    ready.wait();
    let accepted = (0..2).filter(|_| rx.recv().unwrap()).count();
    let running = scan.status().running;
    release.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(accepted, 1);
    assert!(running);
    assert!(!scan.status().running);
}
