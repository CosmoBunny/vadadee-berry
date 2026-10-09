//! Kernel abstraction (WGSL today, by design — no second language yet).
//!
//! A [`VbKernel`] is engine-owned data: name, validated source, declared
//! parameters, and a content hash for the pipeline cache. Execution arrives
//! in milestone 2; this phase covers definition + validation + caching keys.
//!
//! Cache key accounts for: source, language, input/output format, and the
//! workgroup + binding configuration derived from the source.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Kernel source language (WGSL only — see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KernelLanguage {
    Wgsl,
}

/// A validated custom kernel (execution in milestone 2).
#[derive(Debug, Clone)]
pub struct VbKernel {
    pub name: String,
    /// Original WGSL source.
    pub source: String,
    pub language: KernelLanguage,
    /// Declared parameter names with defaults.
    pub params: Vec<(String, f32)>,
    /// Stable pipeline-cache key (source + language + params + formats).
    pub cache_key: u64,
    /// Input/output pixel format tag (today: always `rgba8`).
    pub format: &'static str,
}

impl VbKernel {
    /// Validate + compile the definition (no GPU needed for validation).
    pub fn create(
        name: String,
        language: &str,
        source: String,
        params: Vec<(String, f32)>,
    ) -> Result<Self, String> {
        if name.is_empty() || name.len() > 64 {
            return Err("kernel name must be 1..64 chars".to_string());
        }
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err("kernel name must be [a-zA-Z0-9_-]".to_string());
        }
        let language = match language.trim().to_ascii_lowercase().as_str() {
            "wgsl" => KernelLanguage::Wgsl,
            other => return Err(format!("unsupported kernel language '{other}' (WGSL only)")),
        };
        validate_wgsl_compute(&source)?;
        if params.len() > 32 {
            return Err("at most 32 kernel parameters".to_string());
        }
        let mut clean = Vec::with_capacity(params.len());
        for (k, v) in params {
            if k.is_empty() || k.len() > 64
                || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(format!("bad parameter name '{k}'"));
            }
            if !v.is_finite() {
                return Err(format!("parameter '{k}' must be finite"));
            }
            if clean.iter().any(|(ek, _): &(String, f32)| ek == &k) {
                return Err(format!("duplicate parameter '{k}'"));
            }
            clean.push((k, v));
        }
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        language.hash(&mut hasher);
        for (k, v) in &clean {
            k.hash(&mut hasher);
            v.to_bits().hash(&mut hasher);
        }
        "rgba8".hash(&mut hasher);
        Ok(Self {
            name,
            source,
            language,
            params: clean,
            cache_key: hasher.finish(),
            format: "rgba8",
        })
    }
}

/// Structural WGSL-compute validation (no device needed).
/// Full pipeline compilation happens at first execution (cached by hash).
pub fn validate_wgsl_compute(source: &str) -> Result<(), String> {
    let src = source.trim();
    if src.is_empty() {
        return Err("kernel source is empty".to_string());
    }
    if src.len() > 64 * 1024 {
        return Err("kernel source exceeds 64 KiB".to_string());
    }
    if !src.contains("@compute") {
        return Err("kernel needs a @compute entry point".to_string());
    }
    if !src.contains("fn main") && !src.contains("fn Main") {
        return Err("kernel needs fn main".to_string());
    }
    // Balanced braces/parens (cheap pre-filter before GPU compile).
    let mut depth_brace = 0i32;
    let mut depth_paren = 0i32;
    for c in src.chars() {
        match c {
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            _ => {}
        }
        if depth_brace < 0 || depth_paren < 0 {
            return Err("unbalanced brackets in kernel source".to_string());
        }
    }
    if depth_brace != 0 || depth_paren != 0 {
        return Err("unbalanced brackets in kernel source".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOBEL: &str = r#"
        @group(0) @binding(0) var input_tex: texture_2d<f32>;
        @group(0) @binding(1) var<storage, read_write> out: array<f32>;
        @compute @workgroup_size(8, 8)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            out[0] = 1.0;
        }
    "#;

    #[test]
    fn accepts_compute_rejects_fragments() {
        assert!(VbKernel::create("sobel".into(), "wgsl", SOBEL.into(), vec![]).is_ok());
        assert!(VbKernel::create("x".into(), "glsl", SOBEL.into(), vec![]).is_err());
        assert!(VbKernel::create(
            "x".into(),
            "wgsl",
            "@fragment fn main() -> vec4<f32> { return vec4<f32>(1.0); }".into(),
            vec![]
        )
        .is_err());
        assert!(VbKernel::create("x".into(), "wgsl", "fn main( {".into(), vec![]).is_err());
        assert!(VbKernel::create("".into(), "wgsl", SOBEL.into(), vec![]).is_err());
    }

    #[test]
    fn cache_key_covers_source_and_params() {
        let a = VbKernel::create("k".into(), "wgsl", SOBEL.into(), vec![("r".into(), 1.0)]).unwrap();
        let b = VbKernel::create("k".into(), "wgsl", SOBEL.into(), vec![("r".into(), 2.0)]).unwrap();
        let c = VbKernel::create("k".into(), "wgsl", SOBEL.into(), vec![("r".into(), 1.0)]).unwrap();
        assert_ne!(a.cache_key, b.cache_key);
        assert_eq!(a.cache_key, c.cache_key);
    }
}
