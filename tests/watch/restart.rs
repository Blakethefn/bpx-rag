use std::fs;
use std::time::Duration;

use super::{add_source, env, run, spawn_watch, text, wait_for};

#[test]
fn killed_watcher_is_reported_stopped_and_a_restart_reconciles_offline_changes() {
    let env = env();
    add_source(&env);
    assert!(run(&env, &["watch", "on", "atlas"]).status.success());

    let mut first = spawn_watch(&env);
    let running = wait_for(Duration::from_secs(20), || {
        text(&run(&env, &["watch", "status"])).contains("watching")
    });
    assert!(
        running,
        "the watcher never reported watching: {}",
        text(&run(&env, &["watch", "status"]))
    );
    first.kill().unwrap();
    first.wait().unwrap();

    let report = text(&run(&env, &["watch", "status"]));
    assert!(
        report.contains("not running"),
        "a killed watcher must not look alive: {report}"
    );

    fs::write(
        env.source.join("guide.md"),
        "# Atlas Notes\n\nThe copper meridian protocol governs dawn navigation across the outer reef.\n",
    )
    .unwrap();

    let mut second = spawn_watch(&env);
    let reconciled = wait_for(Duration::from_secs(20), || {
        let found = text(&run(&env, &["search", "copper meridian"]));
        found.contains("copper meridian")
    });
    second.kill().unwrap();
    second.wait().unwrap();
    assert!(
        reconciled,
        "a restarted watcher must reconcile changes made while it was stopped"
    );
}
