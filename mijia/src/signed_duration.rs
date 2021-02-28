use std::fmt::{self, Debug, Formatter, Write};
use std::time::{Duration, SystemTimeError};

/// A duration which may be negative.
///
/// Example:
/// ```
/// use mijia::SignedDuration;
/// use std::time::SystemTime;
///
/// let then = SystemTime::now();
/// let now = SystemTime::now();
/// let offset: SignedDuration = now.duration_since(then).into();
/// ```
#[derive(Clone, Eq, PartialEq)]
pub struct SignedDuration {
    /// Whether this represents a positive duration of time.
    pub positive: bool,
    /// The absolute value of the duration.
    pub duration: Duration,
}

impl From<Duration> for SignedDuration {
    fn from(duration: Duration) -> Self {
        SignedDuration {
            positive: true,
            duration,
        }
    }
}

impl From<Result<Duration, SystemTimeError>> for SignedDuration {
    fn from(result: Result<Duration, SystemTimeError>) -> Self {
        match result {
            Ok(duration) => duration.into(),
            Err(err) => SignedDuration {
                positive: false,
                duration: err.duration(),
            },
        }
    }
}

impl Debug for SignedDuration {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if !self.positive {
            f.write_char('-')?;
        }
        self.duration.fmt(f)
    }
}
