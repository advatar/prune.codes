#[cfg(feature = "surreal")]
use anyhow::Context;

#[cfg(feature = "surreal")]
pub async fn ensure_schema(
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
    embed_dim: usize,
) -> anyhow::Result<()> {
    let raw = include_str!("../schema/prune.surql");
    let rendered = raw.replace("$EMBED_DIM", &embed_dim.to_string());
    apply_schema(db, &rendered).await.context("failed to apply Surreal base schema")?;

    let edges = include_str!("../schema/prune_edges.surql");
    apply_schema(db, edges).await.context("failed to apply Surreal edge schema")?;

    Ok(())
}

async fn apply_schema(
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
    sql: &str,
) -> anyhow::Result<()> {
    for stmt in sql.split(';') {
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
