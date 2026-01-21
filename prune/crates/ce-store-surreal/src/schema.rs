#[cfg(feature = "surreal")]
use anyhow::Context;

#[cfg(feature = "surreal")]
pub async fn ensure_schema(
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
    embed_dim: usize,
) -> anyhow::Result<()> {
    let raw = include_str!("../schema/prune.surql");
    let rendered = raw.replace("$EMBED_DIM", &embed_dim.to_string());
    for stmt in rendered.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() || stmt.starts_with("--") {
            continue;
        }
        let sql = format!("{stmt};");
        if let Err(err) = db.query(sql).await {
            let msg = err.to_string().to_ascii_lowercase();
            let is_duplicate = msg.contains("already exists") || msg.contains("already defined");
            if !is_duplicate {
                return Err(err).context("failed to apply Surreal schema");
            }
        }
    }
    Ok(())
}
