use super::*;

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
