//! Safe-ish wrappers for llama.cpp speculative decoding helpers.
//!
//! `LlamaSpeculativeMtp` stores an opaque llama.cpp state object that refers to
//! a target context and a Gemma MTP draft context. Drop the speculative state
//! before dropping either context.

use std::ptr::NonNull;

use crate::context::LlamaContext;
use crate::llama_batch::LlamaBatch;
use crate::token::LlamaToken;

/// Runtime parameters for Gemma MTP speculative decoding.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::module_name_repetitions)]
pub struct LlamaSpeculativeMtpParams {
    /// Maximum draft tokens requested per target sample.
    pub max_draft_tokens: u32,
    /// Minimum draft tokens required for a draft to be returned.
    pub min_draft_tokens: u32,
    /// Minimum probability required from the draft sampler.
    pub min_draft_probability: f32,
    /// Use llama.cpp backend sampling on the draft context when available.
    pub backend_sampling: bool,
}

impl Default for LlamaSpeculativeMtpParams {
    fn default() -> Self {
        Self {
            max_draft_tokens: 3,
            min_draft_tokens: 0,
            min_draft_probability: 0.0,
            backend_sampling: true,
        }
    }
}

/// Lightweight counters maintained by the Rust wrapper.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub struct LlamaSpeculativeMtpStats {
    /// Number of draft calls issued.
    pub draft_calls: u64,
    /// Number of draft tokens returned by llama.cpp.
    pub drafted_tokens: u64,
    /// Number of accept calls issued after non-empty drafts.
    pub accept_calls: u64,
    /// Number of draft tokens accepted by the target model.
    pub accepted_tokens: u64,
}

/// Errors returned by the MTP bridge.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub enum LlamaSpeculativeMtpError {
    /// Caller passed invalid local parameters before the FFI boundary.
    #[error("{0}")]
    InvalidParams(&'static str),
    /// llama.cpp rejected an argument.
    #[error("llama.cpp speculative MTP bridge rejected an argument")]
    InvalidArgument,
    /// llama.cpp failed to allocate memory.
    #[error("llama.cpp speculative MTP bridge failed to allocate memory")]
    AllocationFailed,
    /// llama.cpp reported an exception while running the bridge.
    #[error("llama.cpp speculative MTP bridge raised an exception")]
    Exception,
    /// llama.cpp returned an unrecognized bridge status.
    #[error("llama.cpp speculative MTP bridge returned status {0}")]
    FfiError(llama_cpp_sys_2::llama_rs_status),
}

/// Opaque Gemma MTP speculative-decoding state.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct LlamaSpeculativeMtp {
    inner: NonNull<llama_cpp_sys_2::llama_rs_speculative>,
    max_draft_tokens: usize,
    stats: LlamaSpeculativeMtpStats,
}

impl LlamaSpeculativeMtp {
    /// Create a Gemma MTP speculator for an already-created target and draft context.
    ///
    /// The draft context must have been created with `with_context_type_mtp()`
    /// and `with_context_other(&target_context)`.
    pub fn new(
        target: &mut LlamaContext<'_>,
        draft: &mut LlamaContext<'_>,
        params: LlamaSpeculativeMtpParams,
        n_seq: u32,
    ) -> Result<Self, LlamaSpeculativeMtpError> {
        if params.max_draft_tokens == 0 {
            return Err(LlamaSpeculativeMtpError::InvalidParams(
                "max_draft_tokens must be greater than zero",
            ));
        }
        if params.min_draft_tokens > params.max_draft_tokens {
            return Err(LlamaSpeculativeMtpError::InvalidParams(
                "min_draft_tokens must be less than or equal to max_draft_tokens",
            ));
        }
        if !params.min_draft_probability.is_finite() || params.min_draft_probability < 0.0 {
            return Err(LlamaSpeculativeMtpError::InvalidParams(
                "min_draft_probability must be finite and non-negative",
            ));
        }
        if n_seq == 0 {
            return Err(LlamaSpeculativeMtpError::InvalidParams(
                "n_seq must be greater than zero",
            ));
        }

        let ffi_params = llama_cpp_sys_2::llama_rs_speculative_mtp_params {
            n_max: i32::try_from(params.max_draft_tokens).map_err(|_| {
                LlamaSpeculativeMtpError::InvalidParams("max_draft_tokens does not fit in i32")
            })?,
            n_min: i32::try_from(params.min_draft_tokens).map_err(|_| {
                LlamaSpeculativeMtpError::InvalidParams("min_draft_tokens does not fit in i32")
            })?,
            p_min: params.min_draft_probability,
            backend_sampling: params.backend_sampling,
        };
        let mut out = std::ptr::null_mut();
        let status = unsafe {
            llama_cpp_sys_2::llama_rs_speculative_mtp_init(
                target.context.as_ptr(),
                draft.context.as_ptr(),
                ffi_params,
                n_seq,
                &mut out,
            )
        };
        map_status(status)?;
        let inner = NonNull::new(out).ok_or(LlamaSpeculativeMtpError::AllocationFailed)?;

        Ok(Self {
            inner,
            max_draft_tokens: usize::try_from(params.max_draft_tokens)
                .expect("u32 fits in usize on supported targets"),
            stats: LlamaSpeculativeMtpStats::default(),
        })
    }

    /// Returns whether this speculator requires target next-token embeddings.
    #[must_use]
    pub fn need_embd_nextn(&self) -> bool {
        unsafe { llama_cpp_sys_2::llama_rs_speculative_need_embd_nextn(self.inner.as_ptr()) }
    }

    /// Optionally notify the speculator that a fresh sequence has begun.
    pub fn begin(
        &mut self,
        seq_id: i32,
        prompt: &[LlamaToken],
    ) -> Result<(), LlamaSpeculativeMtpError> {
        let prompt = to_raw_tokens(prompt);
        let ptr = optional_ptr(&prompt);
        let status = unsafe {
            llama_cpp_sys_2::llama_rs_speculative_begin(
                self.inner.as_ptr(),
                seq_id,
                ptr,
                prompt.len(),
            )
        };
        map_status(status)
    }

    /// Process a target-model batch after a successful target decode.
    pub fn process(&mut self, batch: &LlamaBatch<'_>) -> Result<(), LlamaSpeculativeMtpError> {
        let status = unsafe {
            llama_cpp_sys_2::llama_rs_speculative_process(
                self.inner.as_ptr(),
                &batch.llama_batch,
            )
        };
        map_status(status)
    }

    /// Ask the draft model for speculative tokens following `last_token`.
    pub fn draft(
        &mut self,
        seq_id: i32,
        n_past: i32,
        last_token: LlamaToken,
        prompt: &[LlamaToken],
        n_max_override: Option<u32>,
    ) -> Result<Vec<LlamaToken>, LlamaSpeculativeMtpError> {
        let capacity = n_max_override
            .map(|value| usize::try_from(value).expect("u32 fits in usize on supported targets"))
            .unwrap_or(self.max_draft_tokens)
            .min(self.max_draft_tokens);
        if capacity == 0 {
            return Ok(Vec::new());
        }

        let prompt = to_raw_tokens(prompt);
        let mut output = vec![0; capacity];
        let mut out_count = 0_usize;
        let n_max = i32::try_from(capacity).map_err(|_| {
            LlamaSpeculativeMtpError::InvalidParams("draft capacity does not fit in i32")
        })?;
        let status = unsafe {
            llama_cpp_sys_2::llama_rs_speculative_draft(
                self.inner.as_ptr(),
                seq_id,
                n_past,
                last_token.0,
                optional_ptr(&prompt),
                prompt.len(),
                n_max,
                output.as_mut_ptr(),
                output.len(),
                &mut out_count,
            )
        };
        map_status(status)?;
        output.truncate(out_count);
        self.stats.draft_calls += 1;
        self.stats.drafted_tokens += u64::try_from(output.len()).unwrap_or(u64::MAX);
        Ok(output.into_iter().map(LlamaToken).collect())
    }

    /// Inform the speculator how many draft tokens the target accepted.
    ///
    /// Call this only after a non-empty `draft()` result; llama.cpp uses the
    /// last draft implementation for the sequence to route the acceptance.
    pub fn accept(
        &mut self,
        seq_id: i32,
        n_accepted: u16,
    ) -> Result<(), LlamaSpeculativeMtpError> {
        let status = unsafe {
            llama_cpp_sys_2::llama_rs_speculative_accept(
                self.inner.as_ptr(),
                seq_id,
                n_accepted,
            )
        };
        map_status(status)?;
        self.stats.accept_calls += 1;
        self.stats.accepted_tokens += u64::from(n_accepted);
        Ok(())
    }

    /// Print llama.cpp's internal speculative decoding statistics.
    pub fn print_stats(&self) {
        unsafe { llama_cpp_sys_2::llama_rs_speculative_print_stats(self.inner.as_ptr()) }
    }

    /// Returns wrapper-maintained statistics.
    #[must_use]
    pub fn stats(&self) -> LlamaSpeculativeMtpStats {
        self.stats
    }
}

impl Drop for LlamaSpeculativeMtp {
    fn drop(&mut self) {
        unsafe { llama_cpp_sys_2::llama_rs_speculative_free(self.inner.as_ptr()) }
    }
}

fn map_status(
    status: llama_cpp_sys_2::llama_rs_status,
) -> Result<(), LlamaSpeculativeMtpError> {
    match status {
        llama_cpp_sys_2::LLAMA_RS_STATUS_OK => Ok(()),
        llama_cpp_sys_2::LLAMA_RS_STATUS_INVALID_ARGUMENT => {
            Err(LlamaSpeculativeMtpError::InvalidArgument)
        }
        llama_cpp_sys_2::LLAMA_RS_STATUS_ALLOCATION_FAILED => {
            Err(LlamaSpeculativeMtpError::AllocationFailed)
        }
        llama_cpp_sys_2::LLAMA_RS_STATUS_EXCEPTION => Err(LlamaSpeculativeMtpError::Exception),
        other => Err(LlamaSpeculativeMtpError::FfiError(other)),
    }
}

fn to_raw_tokens(tokens: &[LlamaToken]) -> Vec<llama_cpp_sys_2::llama_token> {
    tokens.iter().map(|token| token.0).collect()
}

fn optional_ptr(tokens: &[llama_cpp_sys_2::llama_token]) -> *const llama_cpp_sys_2::llama_token {
    if tokens.is_empty() {
        std::ptr::null()
    } else {
        tokens.as_ptr()
    }
}
