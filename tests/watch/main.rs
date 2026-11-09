use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::{tempdir, TempDir};

struct Env {
    _temp: TempDir,
    project: PathBuf,
    source: PathBuf,
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
}

fn env() -> Env {
    let temp = tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let project = root.join("project");
    let source = root.join("notes");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("guide.md"),
        "# Atlas Notes\n\nThe cobalt lantern protocol governs night navigation across the outer reef.\n",
    )
    .unwrap();
    Env {
        project,
        source,
        config: root.join("config"),
        data: root.join("data"),
        cache: root.join("cache"),
        _temp: temp,
    }
}

fn command(env: &Env) -> Command {
    let mut command = Command::new(std::env::var("CARGO_BIN_EXE_bpx-rag").unwrap());
    command
        .current_dir(&env.project)
        .env("XDG_CONFIG_HOME", &env.config)
        .env("XDG_DATA_HOME", &env.data)
        .env("XDG_CACHE_HOME", &env.cache)
        .env_remove("HF_HOME");
    command
}

fn run(env: &Env, args: &[&str]) -> Output {
    command(env).args(args).output().unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    condition()
}

fn add_source(env: &Env) {
    let add = run(
        env,
        &["add", env.source.to_str().unwrap(), "--label", "atlas"],
    );
    assert!(add.status.success(), "{}", text(&add));
}

fn spawn_watch(env: &Env) -> Child {
    command(env)
        .args(["watch", "run"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn watch_controls_report_enabled_sources_and_no_running_watcher() {
    let env = env();
    add_source(&env);

    let on = run(&env, &["watch", "enable", "atlas"]);
    assert!(on.status.success(), "{}", text(&on));
    let status = run(&env, &["watch", "status"]);
    assert!(status.status.success(), "{}", text(&status));
    let report = text(&status);
    assert!(report.contains("atlas"), "{report}");
    assert!(report.contains("enabled"), "{report}");
    assert!(report.contains("not running"), "{report}");

    let off = run(&env, &["watch", "disable", "atlas"]);
    assert!(off.status.success(), "{}", text(&off));
    let report = text(&run(&env, &["watch", "status"]));
    assert!(report.contains("disabled"), "{report}");
}

#[test]
fn default_search_shows_freshness_and_marks_an_edited_file_stale_until_synced() {
    let env = env();
    add_source(&env);

    let fresh = text(&run(&env, &["search", "cobalt lantern"]));
    assert!(fresh.contains("freshness: fresh ("), "{fresh}");

    fs::write(
        env.source.join("guide.md"),
        "# Atlas Notes\n\nThe cobalt lantern protocol governs night navigation across the outer reef, revised.\n",
    )
    .unwrap();
    let stale = text(&run(&env, &["search", "cobalt lantern"]));
    assert!(stale.contains("freshness: stale (changed)"), "{stale}");

    let sync = run(&env, &["sync", "--label", "atlas"]);
    assert!(sync.status.success(), "{}", text(&sync));
    let synced = text(&run(&env, &["search", "cobalt lantern"]));
    assert!(synced.contains("freshness: fresh"), "{synced}");
}

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
