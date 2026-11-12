use super::*;

fn install_probe(fail_first: u32) -> Arc<RegisterProbe> {
    let probe = Arc::new(RegisterProbe {
        fail_first: std::sync::atomic::AtomicU32::new(fail_first),
        attempts: std::sync::Mutex::new(Vec::new()),
    });
    REGISTER_PROBE.with(|slot| *slot.borrow_mut() = Some(probe.clone()));
    probe
}

fn attempt_count(probe: &RegisterProbe) -> usize {
    probe.attempts.lock().unwrap().len()
}

fn sleep_until(at: Instant) {
    let now = Instant::now();
    if at > now {
        std::thread::sleep(at - now);
    }
}

#[test]
fn registration_failure_sets_error_retries_with_backoff_and_recovers() {
    let fixture = fixture();
    let probe = install_probe(2);
    let mut engine = Engine::new(&fixture.db, fast_options(), Box::new(|| None)).unwrap();

    engine.refresh_sources().unwrap();
    let entry = &engine.watched["test"];
    assert_eq!(entry.state, SourceState::Error);
    assert!(entry.registration.is_none());
    assert!(
        entry.reconciled_at.is_none(),
        "no reconcile without a watch"
    );
    let message = entry.last_error.clone().unwrap();
    assert!(
        message.starts_with("watch registration failed: "),
        "got: {message}"
    );
    assert!(message.contains("Too many open files"), "got: {message}");
    let first_retry = entry.retry_at.expect("a retry must be scheduled");
    assert_eq!(entry.backoff, Duration::from_millis(200), "backoff doubles");
    let reported = source_status(&fixture.db, "test").unwrap();
    assert_eq!(reported.state, SourceState::Error);
    assert_eq!(reported.last_error.as_deref(), Some(message.as_str()));

    engine.retry_due();
    assert_eq!(attempt_count(&probe), 1);

    sleep_until(first_retry);
    engine.retry_due();
    assert_eq!(attempt_count(&probe), 2);
    let entry = &engine.watched["test"];
    assert_eq!(entry.state, SourceState::Error);
    assert_eq!(entry.backoff, Duration::from_millis(400));
    let second_retry = entry.retry_at.unwrap();

    sleep_until(second_retry);
    engine.retry_due();
    assert_eq!(attempt_count(&probe), 3);
    let entry = &engine.watched["test"];
    assert_eq!(entry.state, SourceState::Watching);
    assert!(entry.registration.is_some());
    assert!(entry.reconciled_at.is_some());
    assert!(entry.last_error.is_none());
    assert!(entry.retry_at.is_none());
    assert_eq!(entry.backoff, fast_options().retry_base);
    assert!(watching(&fixture.db));

    let attempts = probe.attempts.lock().unwrap().clone();
    assert!(attempts[1] - attempts[0] >= Duration::from_millis(100));
    assert!(attempts[2] - attempts[1] >= Duration::from_millis(200));
}

#[test]
fn registration_backoff_is_capped_at_the_maximum() {
    let fixture = fixture();
    let probe = install_probe(u32::MAX);
    let mut engine = Engine::new(&fixture.db, fast_options(), Box::new(|| None)).unwrap();
    engine.refresh_sources().unwrap();

    for _ in 0..4 {
        let retry_at = engine.watched["test"].retry_at.unwrap();
        sleep_until(retry_at);
        engine.retry_due();
    }

    assert_eq!(attempt_count(&probe), 5);
    assert_eq!(engine.watched["test"].state, SourceState::Error);
    assert_eq!(
        engine.watched["test"].backoff,
        fast_options().retry_max,
        "the delay must not grow past retry_max"
    );
}

#[test]
fn watcher_thread_recovers_from_registration_failures_and_then_follows_edits() {
    let fixture = fixture();
    let stop = Arc::new(AtomicBool::new(false));
    let probe = Arc::new(RegisterProbe {
        fail_first: std::sync::atomic::AtomicU32::new(2),
        attempts: std::sync::Mutex::new(Vec::new()),
    });
    let handle = {
        let db = fixture.db.clone();
        let stop = stop.clone();
        let probe = probe.clone();
        std::thread::spawn(move || {
            REGISTER_PROBE.with(|slot| *slot.borrow_mut() = Some(probe));
            run_without_vectors(&db, &fast_options(), &stop)
        })
    };

    assert!(wait_for(Duration::from_secs(10), || watching(&fixture.db)));
    assert_eq!(attempt_count(&probe), 3);
    assert!(
        !handle.is_finished(),
        "registration failures must not end the watcher"
    );

    fs::write(
        fixture.source_dir.join("a.rs"),
        "pub fn after_recovery() -> u8 { 1 }\n",
    )
    .unwrap();
    assert!(wait_for(Duration::from_secs(10), || symbols(&fixture.db)
        == vec!["after_recovery"]));
    assert_eq!(stop_and_join(stop, handle), RunOutcome::Stopped);
}

#[test]
fn watcher_starts_when_the_lease_is_only_held_briefly_by_a_probe() {
    let fixture = fixture();
    let (held_tx, held_rx) = std::sync::mpsc::channel();
    let path = lease_path(&fixture.db);
    let probe = std::thread::spawn(move || {
        let lock = FileLock::try_acquire(&path)
            .unwrap()
            .expect("lease is free");
        held_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        drop(lock);
    });
    held_rx.recv().unwrap();

    let stop = AtomicBool::new(true);
    let outcome = run_without_vectors(&fixture.db, &fast_options(), &stop).unwrap();

    probe.join().unwrap();
    assert_eq!(
        outcome,
        RunOutcome::Stopped,
        "a momentary probe must not be reported as a running watcher"
    );
}

#[test]
fn a_lease_that_stays_held_is_still_reported_as_already_running_after_the_wait() {
    let fixture = fixture();
    let _held = FileLock::acquire(&lease_path(&fixture.db)).unwrap();
    let stop = AtomicBool::new(true);
    let started = Instant::now();

    let outcome = run_without_vectors(&fixture.db, &fast_options(), &stop).unwrap();

    assert!(matches!(outcome, RunOutcome::AlreadyRunning { .. }));
    assert!(
        started.elapsed() >= fast_options().lease_wait,
        "the wait must elapse before reporting"
    );
}
