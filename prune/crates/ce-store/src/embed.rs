use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Mutex;

pub const DEFAULT_MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2;

/// Small wrapper around fastembed.
///
/// fastembed's `TextEmbedding::embed` requires `&mut self`,
/// so we keep the model behind a Mutex for easy shared usage.
pub struct Embedder {
    model: Mutex<TextEmbedding>,
    model_name: String,
    dim: usize,
}

impl Embedder {
    pub fn new(model: EmbeddingModel) -> Result<Self> {
        let model_name = format!("{model:?}");

        // Use ModelInfo to get embedding dimension.
        let info = TextEmbedding::get_model_info(&model)?;
        let dim = info.dim;

        let model = TextEmbedding::try_new(
            InitOptions::new(model).with_show_download_progress(true),
        )?;

        Ok(Self {
            model: Mutex::new(model),
            model_name,
            dim,
        })
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn embed_passages(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let docs: Vec<String> = texts.iter().map(|t| format!("passage: {t}")).collect();
        let mut model = self.model.lock().unwrap();
        Ok(model.embed(docs, None)?)
    }

    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        let mut model = self.model.lock().unwrap();
        let v = model.embed(vec![format!("query: {query}")], None)?;
        Ok(v.into_iter().next().unwrap_or_default())
    }
}
