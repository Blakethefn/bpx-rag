//! Compatibility launcher for installations that still invoke `bpcontext`.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let executable = match std::env::current_exe() {
        Ok(path) => {
            let mut sibling = path.with_file_name("bpx-rag");
            if cfg!(windows) {
                sibling.set_extension("exe");
            }
            sibling
        }
        Err(error) => {
            eprintln!("bpcontext: could not locate bpx-rag: {error}");
            return ExitCode::FAILURE;
        }
    };

    match Command::new(executable)
        .args(std::env::args_os().skip(1))
        .status()
    {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("bpcontext: could not launch bpx-rag: {error}");
            ExitCode::FAILURE
        }
    }
}
