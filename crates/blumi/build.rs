//! Build script: on Apple Silicon, link clang's compiler-rt builtins so the ONNX
//! Runtime CoreML execution provider (the `gpu-coreml` feature, default on
//! macOS-aarch64) resolves `___isPlatformVersionAtLeast` — emitted by CoreML's
//! `@available` checks — at link time. Without it, the **release** link
//! (`cargo install`) fails with "Undefined symbols: ___isPlatformVersionAtLeast"
//! because the release profile links with `-nodefaultlibs` and doesn't pull in
//! compiler-rt automatically.
//!
//! No-op on every other target; harmless when CoreML isn't compiled (a static
//! archive only contributes objects that resolve still-undefined symbols).

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if os != "macos" || arch != "aarch64" {
        return;
    }

    // Locate libclang_rt.osx.a via whichever clang toolchain `cc` will use
    // (Command Line Tools or Xcode). Best-effort: if anything is missing, leave
    // the link untouched (debug builds link fine without it).
    let Ok(out) = Command::new("clang").arg("--print-runtime-dir").output() else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let archive = format!("{dir}/libclang_rt.osx.a");
    if Path::new(&archive).exists() {
        // Add the archive to the final link so the on-demand builtins (incl.
        // ___isPlatformVersionAtLeast) are resolvable for ort's CoreML EP.
        println!("cargo:rustc-link-arg={archive}");
    }
}
