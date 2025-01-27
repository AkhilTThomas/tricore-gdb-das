use std::error::Error;
use std::fmt;

use gdbstub::target::TargetError;

/// Target-specific Fatal Error
#[derive(Debug)]
pub enum TricoreTargetError {
    // ...
    Fatal(String),
}

impl fmt::Display for TricoreTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TricoreTargetError::Fatal(msg) => write!(f, "Fatal error: {}", msg),
        }
    }
}

impl From<TricoreTargetError> for TargetError<&'static str> {
    fn from(error: TricoreTargetError) -> Self {
        match error {
            TricoreTargetError::Fatal(_s) => TargetError::Fatal("Fatal error"),
        }
    }
}

impl Error for TricoreTargetError {}
