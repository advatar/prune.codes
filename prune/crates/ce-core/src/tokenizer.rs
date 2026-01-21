//! Token counting utilities.
//!
//! We use `tiktoken-rs` (OpenAI's tiktoken-compatible BPE) to count tokens for
//! budgeting and reporting.
//!
//! If the configured tokenizer spec can't be resolved, we fall back to the
//! conservative heuristic in `ce_core::util::approx_tokens`.

use anyhow::Result;
use std::fmt;
use tiktoken_rs::{
    cl100k_base_singleton, get_bpe_from_model, o200k_base_singleton,
    o200k_harmony_singleton, p50k_base_singleton, p50k_edit_singleton,
    r50k_base_singleton, CoreBPE,
};

enum BpeHandle {
    Static(&'static CoreBPE),
    Owned(CoreBPE),
}

impl fmt::Debug for BpeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BpeHandle::Static(_) => f.write_str("BpeHandle::Static"),
            BpeHandle::Owned(_) => f.write_str("BpeHandle::Owned"),
        }
    }
}

impl BpeHandle {
    fn count_tokens(&self, text: &str) -> usize {
        match self {
            BpeHandle::Static(bpe) => bpe.encode_with_special_tokens(text).len(),
            BpeHandle::Owned(bpe) => bpe.encode_with_special_tokens(text).len(),
        }
    }
}

/// Token counter used by the packer.
///
/// `spec` may be:
/// - a tiktoken encoding name (recommended): `o200k_base`, `cl100k_base`, ...
/// - a model name (best-effort): `gpt-4o`, `gpt-4.1`, ...
/// - prefixed: `encoding:o200k_base` or `model:gpt-4o`
///
/// If resolution fails, token counting falls back to `approx_tokens`.
#[derive(Debug)]
pub struct TokenCounter {
    spec: String,
    bpe: Option<BpeHandle>,
}

impl TokenCounter {
    pub fn new(spec: &str) -> Self {
        let spec = spec.trim();
        let (kind, name) = split_spec(spec);

        let bpe = match kind {
            SpecKind::Encoding => bpe_from_encoding(name).ok(),
            SpecKind::Model => get_bpe_from_model(name).ok().map(BpeHandle::Owned),
            SpecKind::Auto => bpe_from_encoding(name)
                .ok()
                .or_else(|| get_bpe_from_model(name).ok().map(BpeHandle::Owned)),
        };

        Self {
            spec: spec.to_string(),
            bpe,
        }
    }

    pub fn spec(&self) -> &str {
        &self.spec
    }

    pub fn is_fallback(&self) -> bool {
        self.bpe.is_none()
    }

    /// Count tokens for `text`.
    ///
    /// Falls back to a conservative heuristic if we failed to initialize a tokenizer.
    pub fn count(&self, text: &str) -> usize {
        if let Some(bpe) = &self.bpe {
            bpe.count_tokens(text)
        } else {
            crate::util::approx_tokens(text)
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum SpecKind {
    Auto,
    Encoding,
    Model,
}

fn split_spec(spec: &str) -> (SpecKind, &str) {
    let s = spec.trim();
    if let Some(rest) = s.strip_prefix("encoding:") {
        return (SpecKind::Encoding, rest.trim());
    }
    if let Some(rest) = s.strip_prefix("model:") {
        return (SpecKind::Model, rest.trim());
    }
    (SpecKind::Auto, s)
}

fn bpe_from_encoding(name: &str) -> Result<BpeHandle> {
    let enc = name.trim();
    let h = match enc {
        "o200k_base" => BpeHandle::Static(o200k_base_singleton()),
        "cl100k_base" => BpeHandle::Static(cl100k_base_singleton()),
        "p50k_base" => BpeHandle::Static(p50k_base_singleton()),
        "p50k_edit" => BpeHandle::Static(p50k_edit_singleton()),
        "r50k_base" => BpeHandle::Static(r50k_base_singleton()),
        "o200k_harmony" => BpeHandle::Static(o200k_harmony_singleton()),
        _ => {
            // Not a known encoding.
            return Err(anyhow::anyhow!("unknown tokenizer encoding: {enc}"));
        }
    };
    Ok(h)
}
