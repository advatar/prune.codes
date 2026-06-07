pub mod db;
pub mod embed;
pub mod graph_report;
pub mod hnsw;
pub mod query;
pub mod types;

pub use db::Db;
pub use embed::Embedder;
pub use hnsw::VecIndex;

// Useful structs for CLI/MCP callers
pub use graph_report::{GraphReport, GraphReportOptions};
pub use types::StrategyRecord;
