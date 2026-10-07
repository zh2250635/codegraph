//! codegraph-kernel — native extraction kernel.
//!
//! Replaces ONLY the parse+extract walk inside the parse workers, behind the
//! existing `ExtractionResult` contract. Input `(filePath, content, language)`
//! per file; output flat typed buffers — one boundary crossing per file.
//! Everything downstream (resolution, synthesis, frameworks, MCP) is
//! untouched and consumes the decoded result exactly as before.
//!
//! Calls are synchronous; per-file scheduling belongs to the caller. The
//! standalone C ABI has no Node dependency, while the default NAPI feature
//! retains the optional JavaScript binding. Both share the same core walk.
//!
//! Per-language extraction lives in a dedicated walker module (tsjs/ for
//! typescript/tsx/javascript/jsx) that mirrors the TS extractor for behavioral
//! parity — verified by scripts/kernel-parity.mjs and the §5 gate.

#![deny(clippy::all)]

/// First statement of every recursive walker function (see stack.rs, #1581):
/// once the stack pointer is inside the red zone, stop descending — the
/// latched flag makes `stack::run_guarded` discard the walk and defer the
/// file to wasm. `Default::default()` covers every walker return type in use
/// (`()`, `bool`, `Option<_>`, `String`); a hook returning `false` just sends
/// its caller down the generic child walk, whose own guard returns at once.
macro_rules! stack_guard {
    () => {
        if $crate::stack::exhausted() {
            return ::core::default::Default::default();
        }
    };
}

mod buffers;
mod ccpp;
mod cfnptr;
mod csharp;
mod dart;
mod docstring;
mod ids;
mod go;
mod java;
mod kotlin;
mod langs;
mod lua;
mod php;
mod rlang;
mod ruby;
mod rustlang;
mod scala;
mod stack;
mod swift;
mod textutil;
mod python;
mod tsjs;

pub mod core_api;

#[cfg(feature = "cabi")]
pub mod cabi;

#[cfg(feature = "napi")]
mod node;

#[cfg(feature = "napi")]
pub use node::*;
