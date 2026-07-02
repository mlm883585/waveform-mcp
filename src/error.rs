//! Unified error type for the wave-analyzer-mcp library.
//!
//! All library functions return [`WaveResult<T>`] instead of `Result<T, String>`,
//! enabling structured error matching by category (signal-not-found, parse error,
//! BFS failure, etc.) while preserving human-readable messages via the `Display`
//! trait (auto-implemented by `thiserror`).

use thiserror::Error;

/// Unified error enum for waveform analysis operations.
///
/// Each variant carries enough context for both programmatic matching
/// and human-readable error messages. The `Display` implementation
/// (provided by `thiserror`) produces clear messages like:
///
/// - `"Signal 'TOP.clk' not found in waveform"` (SignalNotFound)
/// - `"Condition expression parse error: unexpected token '+'"` (ConditionParseError)
/// - `"BFS trace error: no path found within depth limit"` (BfsError)
#[derive(Debug, Error)]
pub enum WaveAnalyzerError {
    /// A signal path could not be resolved in the waveform hierarchy.
    #[error("Signal '{path}' not found in waveform")]
    SignalNotFound {
        /// The hierarchical signal path that was searched.
        path: String,
    },

    /// A waveform file has not been loaded (or was closed).
    #[error("Waveform not loaded (id: {id})")]
    WaveformNotLoaded {
        /// The waveform identifier that was requested.
        id: String,
    },

    /// An argument or parameter is invalid.
    #[error("Invalid argument: {message}")]
    InvalidArgument {
        /// Description of the invalid argument.
        message: String,
    },

    /// A condition expression could not be parsed or evaluated.
    #[error("Condition expression parse error: {message}")]
    ConditionParseError {
        /// The parse or evaluation error detail.
        message: String,
    },

    /// BFS root-cause tracing failed or produced no result.
    #[error("BFS trace error: {message}")]
    BfsError {
        /// The BFS-specific error detail.
        message: String,
    },

    /// CDC (cross-clock-domain) analysis encountered an error.
    #[error("CDC analysis error: {message}")]
    CdcError {
        /// The CDC-specific error detail.
        message: String,
    },

    /// Protocol analysis (handshake, clock, pulse, interval) failed.
    #[error("Protocol analysis error: {message}")]
    ProtocolError {
        /// The protocol-specific error detail.
        message: String,
    },

    /// Dependency graph loading or resolution failed.
    #[error("Dependency graph error: {message}")]
    DepsError {
        /// The dependency graph error detail.
        message: String,
    },

    /// A file I/O operation failed.
    #[error("File I/O error for '{path}': {message}")]
    FileError {
        /// The file path that caused the error.
        path: String,
        /// The underlying error message.
        message: String,
    },

    /// Assertion log parsing or processing failed.
    #[error("Assertion log error: {message}")]
    AssertionError {
        /// The assertion-specific error detail.
        message: String,
    },

    /// Catch-all for errors that haven't been classified yet.
    /// Used during the gradual transition from `Result<T, String>`.
    #[error("{0}")]
    Other(String),
}

// --- Conversion helpers for gradual transition ---

impl From<String> for WaveAnalyzerError {
    fn from(s: String) -> Self {
        WaveAnalyzerError::Other(s)
    }
}

impl From<&str> for WaveAnalyzerError {
    fn from(s: &str) -> Self {
        WaveAnalyzerError::Other(s.to_string())
    }
}

/// Reverse conversion: WaveAnalyzerError → String.
///
/// Needed so that callers still returning `Result<T, String>` (not yet migrated)
/// can use `?` with functions that now return `WaveResult<T>`.
/// The `Display` impl (provided by `thiserror`) produces a human-readable message.
impl From<WaveAnalyzerError> for String {
    fn from(e: WaveAnalyzerError) -> Self {
        e.to_string()
    }
}

/// Conversion: WaveAnalyzerError → Cow<'static, str>.
///
/// Needed so that MCP server tools can pass `WaveAnalyzerError` directly to
/// `McpError::invalid_params(e, None)` and `McpError::internal_error(e, None)`,
/// which expect `impl Into<Cow<'static, str>>`.
impl From<WaveAnalyzerError> for std::borrow::Cow<'static, str> {
    fn from(e: WaveAnalyzerError) -> Self {
        std::borrow::Cow::Owned(e.to_string())
    }
}

/// Convenience type alias: `WaveResult<T> = Result<T, WaveAnalyzerError>`.
///
/// Replaces the previous `Result<T, String>` pattern throughout the library.
pub type WaveResult<T> = Result<T, WaveAnalyzerError>;
