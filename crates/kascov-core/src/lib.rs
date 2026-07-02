pub mod detect;
pub mod model;
pub mod node;
pub mod store;
pub mod sync;

pub use model::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rpc error: {0}")]
    Rpc(String),
    #[error("connect error: {0}")]
    Connect(String),
    #[error("node mismatch: {0}")]
    NodeMismatch(String),
    #[error("invalid {what}: {value}")]
    Invalid { what: &'static str, value: String },
}

pub type Result<T> = std::result::Result<T, Error>;
