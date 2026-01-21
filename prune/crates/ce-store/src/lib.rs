pub mod db;
pub mod embed;
pub mod hnsw;
pub mod query;
pub mod types;

pub use db::Db;
pub use embed::Embedder;
pub use hnsw::VecIndex;

// Useful structs for CLI/MCP callers
pub use types::StrategyRecord;
