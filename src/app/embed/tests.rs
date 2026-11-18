use std::cell::Cell;
use std::time::{Duration, Instant};

use anyhow::anyhow;

use crate::config::EmbeddingDevice;

use super::runtime::{MAX_ATTEMPTS, RETRY_COOLDOWN};
use super::{gpu_skip_reason_for, initialize_once, load_with_fallback, EmbedderState};

#[test]
fn cuda_failure_falls_back_to_cpu_once() {
    let mut state = EmbedderState::Uninitialized;
    let attempts = Cell::new(0);
    let initialized = initialize_once(&mut state, Instant::now, || {
        load_with_fallback(EmbeddingDevice::Cuda, true, |device| {
            attempts.set(attempts.get() + 1);
            match device {
                EmbeddingDevice::Cuda => Err(anyhow!("simulated CUDA error 304")),
                EmbeddingDevice::Cpu => Ok("cpu session"),
            }
        })
        .map(|(session, device, _)| (session, device))
        .map_err(|error| error.to_string())
    })
    .expect("CPU fallback should succeed");
    assert_eq!(initialized, Some(("cpu session", EmbeddingDevice::Cpu)));
    assert_eq!(attempts.get(), 2);

    let cached = initialize_once(&mut state, Instant::now, || {
        attempts.set(attempts.get() + 1);
        Err::<(&str, EmbeddingDevice), String>("provider retry must not run".into())
    })
    .expect("ready state should bypass provider initialization");
    assert!(cached.is_none());
    assert_eq!(attempts.get(), 2);
}

#[test]
fn failed_initialization_waits_out_the_cooldown_before_retrying() {
    let mut state = EmbedderState::Uninitialized;
    let start = Instant::now();
    let now = Cell::new(start);
    let clock = || now.get();
    let attempts = Cell::new(0);
    let failing = || {
        attempts.set(attempts.get() + 1);
        Err::<(&str, EmbeddingDevice), String>("RAM spike".into())
    };
    assert!(initialize_once(&mut state, clock, failing).is_err());
    assert_eq!(attempts.get(), 1);

    now.set(start + Duration::from_secs(10));
    assert!(initialize_once(&mut state, clock, failing).is_err());
    assert_eq!(attempts.get(), 1, "no retry inside the cooldown window");

    now.set(start + RETRY_COOLDOWN + Duration::from_secs(1));
    let recovered = initialize_once(&mut state, clock, || {
        attempts.set(attempts.get() + 1);
        Ok(("cpu session", EmbeddingDevice::Cpu))
    });
    assert_eq!(recovered, Ok(Some(("cpu session", EmbeddingDevice::Cpu))));
    assert_eq!(attempts.get(), 2);
}

#[test]
fn cooldown_starts_when_a_slow_failure_is_recorded() {
    let mut state = EmbedderState::Uninitialized;
    let start = Instant::now();
    let now = Cell::new(start);
    let clock = || now.get();
    let attempts = Cell::new(0);
    let slow_failure = || {
        attempts.set(attempts.get() + 1);
        now.set(start + Duration::from_secs(60));
        Err::<(&str, EmbeddingDevice), String>("cold load timed out".into())
    };
    assert!(initialize_once(&mut state, clock, slow_failure).is_err());

    // The failure is recorded at 60s, so 80s is 20s later: still inside the cooldown.
    now.set(start + Duration::from_secs(80));
    assert!(initialize_once(&mut state, clock, slow_failure).is_err());
    assert_eq!(
        attempts.get(),
        1,
        "cooldown must count from the recorded failure"
    );
}

#[test]
fn retries_stop_after_the_attempt_cap() {
    let mut state = EmbedderState::Uninitialized;
    let start = Instant::now();
    let now = Cell::new(start);
    let clock = || now.get();
    let attempts = Cell::new(0);
    for _ in 0..MAX_ATTEMPTS {
        let _ = initialize_once(&mut state, clock, || {
            attempts.set(attempts.get() + 1);
            Err::<(&str, EmbeddingDevice), String>("still failing".into())
        });
        now.set(now.get() + RETRY_COOLDOWN + Duration::from_secs(1));
    }
    assert_eq!(attempts.get(), MAX_ATTEMPTS as usize);

    now.set(now.get() + Duration::from_secs(3600));
    let capped = initialize_once(&mut state, clock, || {
        attempts.set(attempts.get() + 1);
        Ok(("never", EmbeddingDevice::Cpu))
    });
    assert!(capped.is_err(), "the cap keeps the failure sticky");
    assert_eq!(attempts.get(), MAX_ATTEMPTS as usize);
}

#[test]
fn gpu_preflight_skips_unavailable_sandbox_devices() {
    assert_eq!(
        gpu_skip_reason_for(false, None, true, false).as_deref(),
        Some("/dev/nvidia0 is not present")
    );
    assert_eq!(
        gpu_skip_reason_for(true, Some("0"), true, true).as_deref(),
        Some("BPX_NO_GPU=1")
    );
    assert_eq!(gpu_skip_reason_for(false, Some("0"), true, true), None);
}
