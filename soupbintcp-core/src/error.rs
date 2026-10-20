#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error")]
    Io(#[from] std::io::Error),

    #[error("sequence gap: expected {expected}, actual {actual}")]
    SequenceGap { expected: u64, actual: u64 },

    #[error("malformed packet: {0}")]
    MalformedPacket(String),

    #[error("unknown packet: {0}")]
    UnknownPacket(String),

    #[error("login rejected reason: {0}")]
    LoginRejected(String),
}

pub type Result<T> = std::result::Result<T, Error>;
