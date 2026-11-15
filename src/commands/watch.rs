use anyhow::Result;

use crate::app::RagApp;
use crate::cli::WatchAction;

pub(crate) fn run_watch(action: WatchAction) -> Result<()> {
    match action {
        WatchAction::Enable { label } => {
            println!("{}", RagApp::open()?.set_watch(&label, true)?);
            Ok(())
        }
        WatchAction::Disable { label } => {
            println!("{}", RagApp::open()?.set_watch(&label, false)?);
            Ok(())
        }
        WatchAction::Status => {
            println!("{}", RagApp::open()?.watch_status()?);
            Ok(())
        }
        WatchAction::Run => {
            match RagApp::open()?.run_watch()? {
                crate::watch::RunOutcome::Stopped => println!("Watcher stopped."),
                crate::watch::RunOutcome::AlreadyRunning { pid } => println!(
                    "A watcher is already running for this database (pid {}).",
                    pid.map_or_else(|| "unknown".to_string(), |pid| pid.to_string())
                ),
            }
            Ok(())
        }
    }
}
