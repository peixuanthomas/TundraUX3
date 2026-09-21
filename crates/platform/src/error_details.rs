use std::{error::Error, fmt};

/// Cloneable native error metadata, captured before crossing worker boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedIoError {
    pub message: String,
    pub os_error_code: Option<i32>,
    cause: Option<Box<CapturedIoError>>,
}
impl CapturedIoError {
    pub fn capture(error: &(dyn Error + 'static)) -> Self {
        Self::capture_at(error, 0)
    }
    fn capture_at(error: &(dyn Error + 'static), depth: usize) -> Self {
        Self {
            message: error.to_string(),
            os_error_code: error
                .downcast_ref::<std::io::Error>()
                .and_then(std::io::Error::raw_os_error),
            cause: (depth < 15)
                .then(|| error.source())
                .flatten()
                .map(|source| Box::new(Self::capture_at(source, depth + 1))),
        }
    }
}
impl fmt::Display for CapturedIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl Error for CapturedIoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref().map(|cause| cause as _)
    }
}

#[cfg(test)]
#[path = "../tests/unit/error_details/tests.rs"]
mod tests;
