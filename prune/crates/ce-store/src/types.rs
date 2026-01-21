use ce_core::model::FragKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub frag_id: String,
    pub rowid: i64,
    pub path: String,
    pub kind: FragKind,
    pub symbol: Option<String>,
    pub score: f32,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct SemanticHit {
    pub rowid: usize,
    pub distance: f32, // DistCosine: smaller is closer
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeRecord {
    pub recipe_id: i64,
    pub fingerprint: String,
    pub fingerprint_hash: String,
    pub tokens: String,
    pub failure_excerpt: String,
    pub pack_summary: String,
    pub patch_meta: String,
    pub tags: Option<String>,
    pub success_tokens: Option<i64>,
    pub iterations: Option<i64>,
    pub created_at_ms: i64,
}

/// Stored strategy configuration (the DGM “genome”).
///
/// `config_json` is a serialized `ce_core::model::StrategyConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRecord {
    pub strategy_id: String,
    pub name: String,
    pub config_json: String,
    pub parent_id: Option<String>,
    pub score: Option<f64>,
    pub created_at_ms: i64,
}
