use std::fmt;
use std::fmt::{Display};


/// Represents errors that can occur when interacting with a Network Key Storage (nks).
///
/// This enum encapsulates different types of errors that may arise during nks operations,
/// including I/O errors, HashiCorp Vault API errors, initialization errors, and unsupported operations.
/// It is designed to provide a clear and descriptive representation of the error, facilitating
/// error handling and logging.
#[derive(Debug)]
#[repr(C)]
pub enum NksError {
    /// Error related to I/O operations, wrapping a `std::io::Error`.
    Io(std::io::Error),
    /// Error occurring during nks initialization, containing an error message.
    InitializationError(String),
    /// Error indicating that an attempted operation is unsupported, containing a description.
    UnsupportedOperation(String),
}

impl fmt::Display for NksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            NksError::Io(ref err) => write!(f, "Communication error: {}", err),
            NksError::InitializationError(ref msg) => write!(f, "Authentication error: {}", msg),
            NksError::UnsupportedOperation(ref msg) => write!(f, "Device-specific error: {}", msg),

        }
    }
}

/// Enables `NksError` to be treated as a trait object for any error (`dyn std::error::Error`).
///
/// This implementation allows for compatibility with Rust's standard error handling mechanisms,
/// facilitating the propagation and inspection of errors through the `source` method.
impl std::error::Error for NksError {}


/// Enables `NksError` to be treated as a trait object for any error (`dyn std::error::Error`).
///
/// This implementation allows for compatibility with Rust's standard error handling mechanisms,
/// facilitating the propagation and inspection of errors through the `source` method.
impl std::error::Error for NksError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NksError::Io(ref err) => Some(err),
            _ => None,
        }
    }
}


