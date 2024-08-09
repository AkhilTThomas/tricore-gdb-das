use std::error::Error;
use std::fmt;

use gdbstub::target::TargetError;

/// Target-specific Fatal Error
#[derive(Debug)]
pub enum TricoreTargetError {
    // ...
    Fatal(String),
    TriggerRemoveFailed(anyhow::Error),
    Str(&'static str),
    String(String),
}

impl fmt::Display for TricoreTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TricoreTargetError::Fatal(msg) => write!(f, "Fatal error: {}", msg),
            TricoreTargetError::TriggerRemoveFailed(e) => write!(f, "Trigger remove failed: {}", e),
            TricoreTargetError::Str(s) => write!(f, "{}", s),
            TricoreTargetError::String(s) => write!(f, "{}", s),
        }
    }
}

impl From<anyhow::Error> for TricoreTargetError {
    fn from(e: anyhow::Error) -> Self {
        TricoreTargetError::TriggerRemoveFailed(e)
    }
}

impl From<&'static str> for TricoreTargetError {
    fn from(s: &'static str) -> Self {
        TricoreTargetError::Str(s)
    }
}

impl From<String> for TricoreTargetError {
    fn from(s: String) -> Self {
        TricoreTargetError::String(s)
    }
}

impl From<TricoreTargetError> for TargetError<&'static str> {
    fn from(error: TricoreTargetError) -> Self {
        match error {
            TricoreTargetError::Str(s) => TargetError::Fatal(s),
            TricoreTargetError::String(_s) => TargetError::Fatal("String error occurred"),
            TricoreTargetError::Fatal(_s) => TargetError::Fatal("Fatal error occurred"),
            TricoreTargetError::TriggerRemoveFailed(_) => {
                TargetError::Fatal("Trigger remove failed")
            }
        }
    }
}

impl Error for TricoreTargetError {}
