//! Structured errors shared by every layer (port of `operation_result.py`).

use serde::Serialize;
use std::fmt;

/// A user-facing failure: a stable `code` for scripts, a short `title`,
/// a readable `message`, optional technical `detail`, and whether retrying may help.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppError {
    pub code: String,
    pub title: String,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
    pub retryable: bool,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(code: &str, title: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            title: title.to_owned(),
            message: message.into(),
            detail: String::new(),
            retryable: false,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    pub fn unexpected(context: &str, detail: impl fmt::Display) -> Self {
        Self::new(
            "UNEXPECTED_ERROR",
            "Unexpected Error",
            "An unexpected error occurred. The session log has the details.",
        )
        .with_detail(format!("{context}: {detail}"))
    }

    pub fn io(context: &str, err: &std::io::Error) -> Self {
        Self::new("IO_ERROR", "File Error", format!("{context} failed."))
            .with_detail(err.to_string())
    }

    pub fn invalid(code: &str, message: impl Into<String>) -> Self {
        Self::new(code, "Invalid Input", message)
    }

    pub fn not_found(what: &str, name: &str) -> Self {
        Self::new(
            "NOT_FOUND",
            &format!("{what} Not Found"),
            format!("{what} '{name}' does not exist."),
        )
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if !self.detail.is_empty() {
            write!(f, " ({})", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}
