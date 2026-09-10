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
mod tests {
    use super::*;
    #[test]
    fn captured_platform_error_keeps_native_code_and_chain() {
        let native = std::io::Error::from_raw_os_error(13);
        let error = crate::PlatformError::from_io("copy", Some("/private/source".into()), &native);
        assert_eq!(error.raw_os_error(), Some(13));
        let source = error.source().unwrap();
        assert_eq!(source.to_string(), native.to_string());
        assert_eq!(
            source
                .downcast_ref::<CapturedIoError>()
                .unwrap()
                .os_error_code,
            Some(13)
        );
        let nested = std::io::Error::other(error);
        let captured = CapturedIoError::capture(&nested);
        assert!(captured.source().is_some());
    }
}
