//! `blumi accel` — inspect GPU/accelerator detection and wire up local GPU use.
//!
//! `detect` prints the one-line result; `status` adds the bundled embedder's
//! active execution provider + current inference backends; `doctor` adds
//! copy-paste setup hints for running embeddings/LLMs on the GPU via a local
//! OpenAI-compatible server (Apple MLX, Ollama, vLLM/llama.cpp).

use crate::AccelCmd;
use blumi_config::BlumiConfig;
use blumi_llm::accel::{self, Accelerator};

pub fn run(action: AccelCmd, config: &BlumiConfig) -> anyhow::Result<()> {
    match action {
        AccelCmd::Detect => {
            println!("{}", accel::detect().as_str());
        }
        AccelCmd::Status => print_status(config),
        AccelCmd::Doctor => {
            print_status(config);
            print_doctor();
        }
    }
    Ok(())
}

fn print_status(config: &BlumiConfig) {
    let detected = accel::detect();
    let compiled = accel::compiled_gpu_providers();

    println!("Accelerator");
    println!(
        "  detected hardware : {} ({})",
        detected.as_str(),
        detected.label()
    );
    println!(
        "  compiled GPU EPs  : {}",
        if compiled.is_empty() {
            "none (CPU-only build)".to_string()
        } else {
            compiled.join(", ")
        }
    );

    let eb = &config.embeddings;
    if !eb.enabled {
        println!("  embeddings        : disabled (FTS5 keyword fallback)");
    } else if eb.backend == "local" {
        let ep = accel::embeddings_accelerator(&config.acceleration);
        println!(
            "  embeddings        : bundled ONNX on {} ({})",
            ep.as_str(),
            ep.label()
        );
        if ep == Accelerator::Cpu && detected != Accelerator::Cpu {
            println!(
                "       ↳ a GPU was detected but the embedder is on CPU — see `blumi accel doctor`"
            );
        }
    } else {
        println!(
            "  embeddings        : backend '{}' (provider '{}')",
            eb.backend, eb.provider
        );
    }

    println!(
        "  llm provider      : {}{}",
        config.llm.provider,
        local_gpu_hint(&config.llm.provider)
    );
    let brain = if config.brain.provider.is_empty() {
        "(reuses main)"
    } else {
        config.brain.provider.as_str()
    };
    println!("  brain provider    : {brain} [{}]", config.brain.mode);
    println!(
        "  config            : acceleration.mode='{}', embeddings_accel='{}'",
        config.acceleration.mode, config.acceleration.embeddings_accel
    );
}

fn local_gpu_hint(provider: &str) -> &'static str {
    match provider {
        "local-mlx" => "  ← Apple MLX server",
        "local-cuda" => "  ← CUDA server",
        "ollama" => "  ← Ollama (GPU if available)",
        _ => "",
    }
}

fn print_doctor() {
    let detected = accel::detect();
    let compiled = accel::compiled_gpu_providers();

    println!();
    println!("Setup hints");
    match detected {
        Accelerator::AppleMetal => {
            if compiled.contains(&"apple-coreml") {
                println!(
                    "  ✓ Apple CoreML is compiled in — the bundled embedder uses the GPU/ANE automatically."
                );
            } else {
                println!("  • Apple Silicon detected, but this binary lacks CoreML. Rebuild with:");
                println!("      cargo build --release --features gpu-coreml");
                println!("    (the official install.sh enables it on Apple Silicon by default.)");
            }
            println!("  • Run an LLM/embeddings on the GPU via an MLX server (OpenAI-compatible):");
            println!(
                "      pip install mlx-lm && mlx_lm.server --model mlx-community/Qwen2.5-Coder-7B-Instruct-4bit --port 8080"
            );
        }
        Accelerator::Cuda => {
            if compiled.contains(&"cuda") {
                println!(
                    "  ✓ CUDA is compiled in — the bundled embedder uses the GPU automatically."
                );
            } else {
                println!("  • NVIDIA GPU detected, but CUDA is opt-in. Either rebuild:");
                println!("      cargo build --release --features gpu-cuda    # needs CUDA toolkit + driver");
                println!("    or (lighter) run a local CUDA server and point blumi at it:");
                println!("      ollama serve            # auto-GPU, OpenAI-compatible on :11434");
                println!("      # or vLLM / llama.cpp on :8000  → provider 'local-cuda'");
            }
        }
        Accelerator::Cpu => {
            println!("  • No GPU detected — the bundled embedder runs on CPU (fully functional, slower).");
            println!("  • With an NVIDIA GPU: rebuild with --features gpu-cuda, or run a local GPU server");
            println!("    (Ollama / vLLM / llama.cpp) and set the providers below.");
        }
    }

    println!();
    println!("  Local-GPU-server providers (already configured — just start the server):");
    println!("    local-mlx  → http://localhost:8080/v1    (Apple MLX)");
    println!("    local-cuda → http://localhost:8000/v1    (vLLM / llama.cpp / TGI)");
    println!("    ollama     → http://localhost:11434/v1   (Ollama, auto-GPU)");
    println!();
    println!("  Point blumi at one by editing ~/.blumi/settings.json, e.g.:");
    println!("    \"llm\":        {{ \"provider\": \"local-mlx\", \"model\": \"mlx-community/Qwen2.5-Coder-7B-Instruct-4bit\" }}");
    println!("    \"embeddings\": {{ \"backend\": \"openai\", \"provider\": \"local-mlx\", \"model\": \"mlx-community/bge-small-en-v1.5\", \"dim\": 384 }}");
    println!(
        "  (or offload embeddings to a GPU peer: \"embeddings\": {{ \"backend\": \"grid\" }})"
    );
}
