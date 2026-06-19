use thiserror::Error;

#[derive(Debug, Error)]
pub enum MrtError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("MRT record too short (need {need}, have {have})")]
    TooShort { need: usize, have: usize },

    #[error("unsupported MRT type {mrt_type}/{subtype}")]
    UnsupportedType { mrt_type: u16, subtype: u16 },

    #[error("BGP parse error: {0}")]
    BgpParse(#[from] rbmp_core::error::Error),

    #[error("invalid prefix length {0} for address family")]
    InvalidPrefixLen(u8),
}
