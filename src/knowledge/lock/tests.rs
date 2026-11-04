use super::*;
use tempfile::tempdir;

#[test]
fn try_acquire_is_none_while_another_handle_holds_the_lock() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("knowledge.db.write.lock");
    let held = FileLock::acquire(&path).unwrap();
    assert!(FileLock::try_acquire(&path).unwrap().is_none());
    drop(held);
    assert!(FileLock::try_acquire(&path).unwrap().is_some());
}

#[test]
fn acquire_waits_for_the_holder_to_release() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("knowledge.db.write.lock");
    let held = FileLock::acquire(&path).unwrap();
    let waiter_path = path.clone();
    let waiter = std::thread::spawn(move || FileLock::acquire(&waiter_path).is_ok());
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(
        !waiter.is_finished(),
        "acquire must block while the lock is held"
    );
    drop(held);
    assert!(waiter.join().unwrap());
}

#[test]
fn writer_lock_sits_beside_the_database() {
    assert_eq!(
        writer_lock_path(Path::new("/data/knowledge.db")),
        PathBuf::from("/data/knowledge.db.write.lock")
    );
}
