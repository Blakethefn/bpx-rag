use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};

use crate::config::{Config, EmbeddingDevice};
use crate::embed::Embedder;

pub(in crate::app) const RETRY_COOLDOWN: Duration = Duration::from_secs(30);
pub(in crate::app) const MAX_ATTEMPTS: u32 = 3;

pub(in crate::app) enum EmbedderState {
    Uninitialized,
    GpuReady,
    CpuReady,
    Unavailable {
        error: String,
        failed_at: Instant,
        attempts: u32,
    },
}

pub(in crate::app) fn load_cached_embedder(
    config: &Config,
    model_dir: &std::path::Path,
) -> Result<(Embedder, EmbeddingDevice)> {
    let load = |device| {
        Embedder::load_cached_with_device(
            model_dir,
            &config.embeddings.model,
            config.embeddings.batch_size,
            config.embeddings.intra_threads,
            device,
        )
    };
    let gpu_reason = (config.embeddings.device == EmbeddingDevice::Cuda)
        .then(gpu_skip_reason)
        .flatten();
    if let Some(reason) = gpu_reason.as_deref() {
        eprintln!("[bpx-rag] CUDA initialization skipped ({reason}); trying CPU ONNX.");
    }
    let try_gpu = config.embeddings.device == EmbeddingDevice::Cuda && gpu_reason.is_none();
    match load_with_fallback(config.embeddings.device, try_gpu, load) {
        Ok((embedder, device, gpu_error)) => {
            if let Some(error) = gpu_error {
                eprintln!("[bpx-rag] CUDA initialization failed ({error}); using CPU ONNX.");
            }
            Ok((embedder, device))
        }
        Err(error) => Err(anyhow!(error)),
    }
}

fn gpu_skip_reason() -> Option<String> {
    let disabled = std::env::var("BPX_NO_GPU").is_ok_and(|value| value == "1");
    let visible = std::env::var("CUDA_VISIBLE_DEVICES").ok();
    let device_node_present =
        !cfg!(target_os = "linux") || std::path::Path::new("/dev/nvidia0").exists();
    gpu_skip_reason_for(
        disabled,
        visible.as_deref(),
        cfg!(feature = "gpu-cuda"),
        device_node_present,
    )
}

pub(in crate::app) fn gpu_skip_reason_for(
    disabled: bool,
    visible_devices: Option<&str>,
    cuda_compiled: bool,
    device_node_present: bool,
) -> Option<String> {
    if disabled {
        return Some("BPX_NO_GPU=1".into());
    }
    if visible_devices.is_some_and(|value| value.trim().is_empty() || value == "-1") {
        return Some("CUDA_VISIBLE_DEVICES disables all devices".into());
    }
    if !cuda_compiled {
        return Some("this binary was built without gpu-cuda support".into());
    }
    if !device_node_present {
        return Some("/dev/nvidia0 is not present".into());
    }
    None
}

pub(in crate::app) fn load_with_fallback<T>(
    requested: EmbeddingDevice,
    try_gpu: bool,
    mut load: impl FnMut(EmbeddingDevice) -> Result<T>,
) -> std::result::Result<(T, EmbeddingDevice, Option<String>), String> {
    if requested == EmbeddingDevice::Cuda && try_gpu {
        return match load(EmbeddingDevice::Cuda) {
            Ok(value) => Ok((value, EmbeddingDevice::Cuda, None)),
            Err(gpu_error) => load(EmbeddingDevice::Cpu)
                .map(|value| (value, EmbeddingDevice::Cpu, Some(gpu_error.to_string())))
                .map_err(|cpu_error| {
                    format!(
                        "The local embedding model could not load on CUDA ({gpu_error}); CPU ONNX fallback also failed ({cpu_error}); using keyword retrieval."
                    )
                }),
        };
    }
    load(EmbeddingDevice::Cpu)
        .map(|value| (value, EmbeddingDevice::Cpu, None))
        .map_err(|error| {
            format!("The local embedding model could not load; using keyword retrieval: {error}")
        })
}

/// Initializes the embedder once it is ready. A failed attempt is retried
/// only after `RETRY_COOLDOWN` (measured from when the failure is recorded)
/// and at most `MAX_ATTEMPTS` times in total.
pub(in crate::app) fn initialize_once<T>(
    state: &mut EmbedderState,
    clock: impl Fn() -> Instant,
    initialize: impl FnOnce() -> Result<(T, EmbeddingDevice), String>,
) -> Result<Option<(T, EmbeddingDevice)>, String> {
    let attempts = match state {
        EmbedderState::GpuReady | EmbedderState::CpuReady => return Ok(None),
        EmbedderState::Unavailable {
            error,
            failed_at,
            attempts,
        } => {
            let cooling_down = clock().saturating_duration_since(*failed_at) < RETRY_COOLDOWN;
            if cooling_down || *attempts >= MAX_ATTEMPTS {
                return Err(error.clone());
            }
            *attempts
        }
        EmbedderState::Uninitialized => 0,
    };
    match initialize() {
        Ok((embedder, device)) => {
            *state = ready_state(device);
            Ok(Some((embedder, device)))
        }
        Err(error) => {
            *state = EmbedderState::Unavailable {
                error: error.clone(),
                failed_at: clock(),
                attempts: attempts + 1,
            };
            Err(error)
        }
    }
}

pub(in crate::app) fn ready_state(device: EmbeddingDevice) -> EmbedderState {
    match device {
        EmbeddingDevice::Cuda => EmbedderState::GpuReady,
        EmbeddingDevice::Cpu => EmbedderState::CpuReady,
    }
}
