pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InsufficientSpace,
    OutOfBounds,
    Misaligned,
    InvalidUtf8,
    InvalidPivot,
}

// region:    --- Error Boilerplate

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::InsufficientSpace => write!(f, "Insufficient space available in the buffer"),
            Error::OutOfBounds => write!(f, "Pointer exceeds the bounds"),
            Error::Misaligned => write!(f, "Pointer is Misaligned"),
            Error::InvalidUtf8 => write!(f, "Invalid UTF8"),
            Error::InvalidPivot => write!(f, "Table Pivot Value is invalid"),

        }
    }
}

impl std::error::Error for Error {}

// endregion: --- Error Boilerplate