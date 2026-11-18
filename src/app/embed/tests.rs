use std::cell::Cell;

use anyhow::anyhow;

use crate::config::EmbeddingDevice;

use super::{gpu_skip_reason_for, initialize_once, load_with_fallback, EmbedderState};

#[test]
fn cuda_failure_falls_back_to_cpu_once() {
    let mut state = EmbedderState::Uninitialized;
    let attempts = Cell::new(0);
    let initialized = initialize_once(&mut state, || {
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

    let cached = initialize_once(&mut state, || {
        attempts.set(attempts.get() + 1);
        Err::<(&str, EmbeddingDevice), String>("provider retry must not run".into())
    })
    .expect("ready state should bypass provider initialization");
    assert!(cached.is_none());
    assert_eq!(attempts.get(), 2);
}

#[test]
fn failed_cpu_fallback_is_sticky() {
    let mut state = EmbedderState::Uninitialized;
    let attempts = Cell::new(0);
    let first = initialize_once(&mut state, || {
        load_with_fallback(EmbeddingDevice::Cuda, true, |_| {
            attempts.set(attempts.get() + 1);
            Err::<(), _>(anyhow!("simulated provider failure"))
        })
        .map(|(session, device, _)| (session, device))
        .map_err(|error| error.to_string())
    });
    assert!(first.is_err());
    assert_eq!(attempts.get(), 2);

    let second = initialize_once(&mut state, || {
        attempts.set(attempts.get() + 1);
        Ok(((), EmbeddingDevice::Cpu))
    });
    assert!(second.is_err());
    assert_eq!(attempts.get(), 2);
}

#[test]
fn gpu_preflight_skips_unavailable_sandbox_devices() {
    assert_eq!(
        gpu_skip_reason_for(false, None, true, false).as_deref(),
        Some("/dev/nvidia0 is not accessible")
    );
    assert_eq!(
        gpu_skip_reason_for(true, Some("0"), true, true).as_deref(),
        Some("BPX_NO_GPU=1")
    );
    assert_eq!(gpu_skip_reason_for(false, Some("0"), true, true), None);
}
