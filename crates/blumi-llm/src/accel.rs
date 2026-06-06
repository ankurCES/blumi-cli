//! Runtime accelerator detection + ONNX execution-provider selection.
//!
//! [`detect`] reports the best accelerator available on this host — Apple
//! CoreML/Metal on Apple Silicon, NVIDIA CUDA on a CUDA box, else CPU. The
//! bundled embedder feeds [`execution_providers`] into fastembed's
//! `InitOptions`; ort always appends a CPU provider and silently falls back, so
//! a missing or feature-disabled GPU degrades to CPU rather than failing.
//!
//! Detection is dependency-light: Apple Silicon is a compile-time fact
//! (`target_os = "macos"` + `target_arch = "aarch64"`); CUDA is a best-effort
//! runtime probe (`nvidia-smi` / driver library on disk). Results are cached.

use std::sync::OnceLock;

/// A detected (or explicitly configured) compute accelerator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Accelerator {
    /// Apple Silicon GPU + Neural Engine via CoreML / Metal.
    #[serde(rename = "apple-coreml")]
    AppleMetal,
    /// NVIDIA GPU via CUDA.
    #[serde(rename = "cuda")]
    Cuda,
    /// CPU — always available.
    #[serde(rename = "cpu")]
    Cpu,
}

impl Accelerator {
    /// Stable machine string (matches the serde tag): used in JSON/status.
    pub fn as_str(self) -> &'static str {
        match self {
            Accelerator::AppleMetal => "apple-coreml",
            Accelerator::Cuda => "cuda",
            Accelerator::Cpu => "cpu",
        }
    }

    /// Human-friendly label for status lines and `blumi accel doctor`.
    pub fn label(self) -> &'static str {
        match self {
            Accelerator::AppleMetal => "Apple CoreML (GPU/ANE)",
            Accelerator::Cuda => "NVIDIA CUDA",
            Accelerator::Cpu => "CPU",
        }
    }

    /// Rank for "which peer is strongest" (Cuda > AppleMetal > Cpu).
    pub fn rank(self) -> u8 {
        match self {
            Accelerator::Cuda => 2,
            Accelerator::AppleMetal => 1,
            Accelerator::Cpu => 0,
        }
    }
}

impl std::fmt::Display for Accelerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Detect the best accelerator on this host (cached after the first call).
pub fn detect() -> Accelerator {
    static CACHE: OnceLock<Accelerator> = OnceLock::new();
    *CACHE.get_or_init(detect_uncached)
}

fn detect_uncached() -> Accelerator {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        // Every Apple Silicon Mac has a Metal GPU + Neural Engine; no probe needed.
        Accelerator::AppleMetal
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        if cuda_present() {
            Accelerator::Cuda
        } else {
            Accelerator::Cpu
        }
    }
}

/// Best-effort, panic-free probe for a usable NVIDIA CUDA stack. Compiled only
/// where Apple Silicon isn't a certainty.
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn cuda_present() -> bool {
    // `nvidia-smi -L` exits 0 and lists devices when a driver + GPU are present.
    let smi = std::process::Command::new("nvidia-smi")
        .arg("-L")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    smi || libcuda_on_disk()
}

/// Driver-library fallback (covers headless / driver-only boxes without the CLI).
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn libcuda_on_disk() -> bool {
    #[cfg(target_os = "linux")]
    {
        const PATHS: &[&str] = &[
            "/usr/lib/x86_64-linux-gnu/libcuda.so.1",
            "/usr/lib/libcuda.so.1",
            "/usr/lib64/libcuda.so.1",
            "/usr/local/cuda/lib64/libcuda.so",
        ];
        PATHS.iter().any(|p| std::path::Path::new(p).exists())
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Resolve the accelerator honoring `acceleration.mode`, then clamp to what was
/// actually compiled in (a provider whose cargo feature is absent degrades to
/// CPU with a warning rather than silently mis-reporting GPU use).
pub fn detect_with_override(cfg: &blumi_config::AccelerationConfig) -> Accelerator {
    let requested = match cfg.mode.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => detect(),
        "cpu" => Accelerator::Cpu,
        "apple" | "metal" | "coreml" => Accelerator::AppleMetal,
        "cuda" | "nvidia" => Accelerator::Cuda,
        other => {
            tracing::warn!("unknown acceleration.mode '{other}'; using auto-detect");
            detect()
        }
    };
    clamp_to_compiled(requested)
}

/// The accelerator the bundled embedder should run on: `embeddings_accel`
/// overrides `mode` when set to anything other than "auto".
pub fn embeddings_accelerator(cfg: &blumi_config::AccelerationConfig) -> Accelerator {
    match cfg.embeddings_accel.trim().to_ascii_lowercase().as_str() {
        "" | "auto" => detect_with_override(cfg),
        "cpu" => Accelerator::Cpu,
        "apple" | "metal" | "coreml" => clamp_to_compiled(Accelerator::AppleMetal),
        "cuda" | "nvidia" => clamp_to_compiled(Accelerator::Cuda),
        other => {
            tracing::warn!("unknown acceleration.embeddings_accel '{other}'; following mode");
            detect_with_override(cfg)
        }
    }
}

/// Which GPU execution providers were compiled into THIS build (reflects the
/// `blumi-llm` cargo features actually enabled — `gpu-coreml` / `gpu-cuda`).
/// Reported by `blumi accel` so users can tell whether a GPU path is even
/// available in their binary.
pub fn compiled_gpu_providers() -> &'static [&'static str] {
    match (cfg!(feature = "gpu-coreml"), cfg!(feature = "gpu-cuda")) {
        (true, true) => &["apple-coreml", "cuda"],
        (true, false) => &["apple-coreml"],
        (false, true) => &["cuda"],
        (false, false) => &[],
    }
}

/// Downgrade to CPU when the requested provider's cargo feature isn't compiled.
fn clamp_to_compiled(acc: Accelerator) -> Accelerator {
    match acc {
        Accelerator::AppleMetal if !cfg!(feature = "gpu-coreml") => {
            tracing::warn!(
                "Apple acceleration requested but this build lacks the `gpu-coreml` feature; \
                 falling back to CPU"
            );
            Accelerator::Cpu
        }
        Accelerator::Cuda if !cfg!(feature = "gpu-cuda") => {
            tracing::warn!(
                "CUDA acceleration requested but this build lacks the `gpu-cuda` feature; \
                 rebuild with `--features gpu-cuda` or point embeddings at a local GPU server. \
                 Falling back to CPU"
            );
            Accelerator::Cpu
        }
        other => other,
    }
}

/// Build the ONNX Runtime execution-provider list for `acc`. ort appends a CPU
/// provider and silently falls back, so an unavailable GPU is safe. Compiled
/// only alongside the embedder (its sole consumer).
#[cfg(feature = "local-embeddings")]
pub fn execution_providers(acc: Accelerator) -> Vec<fastembed::ExecutionProviderDispatch> {
    match acc {
        #[cfg(feature = "gpu-coreml")]
        Accelerator::AppleMetal => {
            vec![ort::execution_providers::CoreMLExecutionProvider::default().build()]
        }
        #[cfg(feature = "gpu-cuda")]
        Accelerator::Cuda => {
            vec![ort::execution_providers::CUDAExecutionProvider::default().build()]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_is_stable_and_known() {
        let a = detect();
        assert_eq!(a, detect(), "detection must be cached/stable");
        assert!(matches!(
            a,
            Accelerator::AppleMetal | Accelerator::Cuda | Accelerator::Cpu
        ));
    }

    #[test]
    fn as_str_matches_serde_and_ranks_order() {
        assert_eq!(Accelerator::AppleMetal.as_str(), "apple-coreml");
        assert_eq!(Accelerator::Cuda.as_str(), "cuda");
        assert_eq!(Accelerator::Cpu.as_str(), "cpu");
        assert!(Accelerator::Cuda.rank() > Accelerator::AppleMetal.rank());
        assert!(Accelerator::AppleMetal.rank() > Accelerator::Cpu.rank());
    }

    #[test]
    fn override_cpu_forces_cpu() {
        let cfg = blumi_config::AccelerationConfig {
            mode: "cpu".into(),
            embeddings_accel: "auto".into(),
        };
        assert_eq!(detect_with_override(&cfg), Accelerator::Cpu);
        assert_eq!(embeddings_accelerator(&cfg), Accelerator::Cpu);
    }

    #[test]
    fn unknown_mode_falls_back_to_detect() {
        let cfg = blumi_config::AccelerationConfig {
            mode: "wat".into(),
            embeddings_accel: "auto".into(),
        };
        // Should not panic; resolves to whatever detect() + clamp yields.
        let _ = detect_with_override(&cfg);
    }
}
