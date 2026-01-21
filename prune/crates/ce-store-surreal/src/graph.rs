use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet, VecDeque};
use surrealdb::engine::any::Any;
use surrealdb::sql::Thing;
use surrealdb::Surreal;

fn strip_prefix(table: &str, id: &str) -> String {
    id.trim_start_matches(&format!("{table}:"))
        .trim_matches('"')
        .to_string()
}

fn unique_ids(table: &str, ids: Vec<Thing>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for id in ids {
        let raw = strip_prefix(table, &id.to_string());
        if seen.insert(raw.clone()) {
            out.push(raw);
        }
    }
    out
}

fn ordered_ids(table: &str, ids: Vec<Thing>) -> Vec<String> {
    ids.into_iter()
        .map(|id| strip_prefix(table, &id.to_string()))
        .filter(|id| !id.is_empty())
        .collect()
}

pub async fn collect_file_neighborhood(
    db: &Surreal<Any>,
    repo_id: &str,
    file_id: &str,
    max_hops: u32,
) -> Result<Vec<String>> {
    let file_id = strip_prefix("file", file_id);
    let sql = format!(
        r#"
        RETURN type::thing("file", $file_id).{{..{max_hops}+collect}}(
            ->imports[WHERE repo_id = $repo_id]->file
        );
    "#
    );

    let mut res = db
        .query(&sql)
        .bind(("file_id", file_id.clone()))
        .bind(("repo_id", repo_id.to_string()))
        .await
        .context("collect_file_neighborhood query failed")?;

    let ids: Vec<Thing> = match res.take(0) {
        Ok(ids) => ids,
        Err(_) => {
            let mut res = db
                .query(&sql)
                .bind(("file_id", file_id.clone()))
                .bind(("repo_id", repo_id.to_string()))
                .await
                .context("collect_file_neighborhood retry failed")?;
            let id: Option<Thing> = res.take(0)?;
            id.map(|id| vec![id]).unwrap_or_default()
        }
    };

    let mut out = unique_ids("file", ids);
    let root = strip_prefix("file", &file_id);
    if !root.is_empty() && !out.contains(&root) {
        out.push(root);
    }
    Ok(out)
}

pub async fn collect_frag_neighborhood(
    db: &Surreal<Any>,
    repo_id: &str,
    frag_id: &str,
    max_hops: u32,
    etypes: &[String],
) -> Result<Vec<String>> {
    let frag_id = strip_prefix("frag", frag_id);
    let sql = format!(
        r#"
        RETURN type::thing("frag", $frag_id).{{..{max_hops}+collect}}(
            ->rel[WHERE repo_id = $repo_id AND etype INSIDE $etypes]->frag
        );
    "#
    );

    let mut res = db
        .query(&sql)
        .bind(("repo_id", repo_id.to_string()))
        .bind(("frag_id", frag_id.clone()))
        .bind(("etypes", etypes.to_vec()))
        .await
        .context("collect_frag_neighborhood query failed")?;

    let ids: Vec<Thing> = match res.take(0) {
        Ok(ids) => ids,
        Err(_) => {
            let mut res = db
                .query(&sql)
                .bind(("repo_id", repo_id.to_string()))
                .bind(("frag_id", frag_id.clone()))
                .bind(("etypes", etypes.to_vec()))
                .await
                .context("collect_frag_neighborhood retry failed")?;
            let id: Option<Thing> = res.take(0)?;
            id.map(|id| vec![id]).unwrap_or_default()
        }
    };

    let mut out = unique_ids("frag", ids);
    let root = strip_prefix("frag", &frag_id);
    if !root.is_empty() && !out.contains(&root) {
        out.push(root);
    }
    Ok(out)
}

pub async fn shortest_path_frags(
    db: &Surreal<Any>,
    repo_id: &str,
    from_id: &str,
    to_id: &str,
    etypes: &[String],
    max_hops: u32,
) -> Result<Vec<String>> {
    let from_id = strip_prefix("frag", from_id);
    let to_id = strip_prefix("frag", to_id);
    let sql = format!(
        r#"
        LET $to = type::thing("frag", $to_id);
        RETURN type::thing("frag", $from_id).{{..{max_hops}+shortest=$to+inclusive}}(
            ->rel[WHERE repo_id = $repo_id AND etype INSIDE $etypes]->frag
        );
    "#
    );

    let mut res = db
        .query(&sql)
        .bind(("repo_id", repo_id.to_string()))
        .bind(("from_id", from_id.clone()))
        .bind(("to_id", to_id.clone()))
        .bind(("etypes", etypes.to_vec()))
        .await
        .context("shortest_path_frags query failed")?;

    let ids: Vec<Thing> = match res.take(0) {
        Ok(ids) => ids,
        Err(_) => {
            let mut res = db
                .query(&sql)
                .bind(("repo_id", repo_id.to_string()))
                .bind(("from_id", from_id.clone()))
                .bind(("to_id", to_id.clone()))
                .bind(("etypes", etypes.to_vec()))
                .await
                .context("shortest_path_frags retry failed")?;
            let id: Option<Thing> = res.take(0)?;
            id.map(|id| vec![id]).unwrap_or_default()
        }
    };

    let mut out = ordered_ids("frag", ids);
    if !out.is_empty() {
        return Ok(out);
    }

    let mut prev: HashMap<String, String> = HashMap::new();
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();
    let max_hops = max_hops as usize;

    visited.insert(from_id.clone());
    queue.push_back((from_id.clone(), 0));

    while let Some((node, depth)) = queue.pop_front() {
        if node == to_id {
            break;
        }
        if depth >= max_hops {
            continue;
        }

        let sql = "SELECT out FROM rel WHERE repo_id = $repo_id AND etype INSIDE $etypes AND in = $node";
        let mut res = db
            .query(sql)
            .bind(("repo_id", repo_id.to_string()))
            .bind(("etypes", etypes.to_vec()))
            .bind(("node", Thing::from(("frag", node.as_str()))))
            .await
            .context("shortest_path_frags fallback query failed")?;
        #[derive(serde::Deserialize)]
        struct Row {
            out: Thing,
        }
        let rows: Vec<Row> = res.take(0)?;
        for row in rows {
            let next = strip_prefix("frag", &row.out.to_string());
            if visited.insert(next.clone()) {
                prev.insert(next.clone(), node.clone());
                queue.push_back((next, depth + 1));
            }
        }
    }

    if !visited.contains(&to_id) {
        return Ok(out);
    }

    let mut path: Vec<String> = Vec::new();
    let mut cur = to_id.clone();
    path.push(cur.clone());
    while let Some(parent) = prev.get(&cur) {
        path.push(parent.clone());
        cur = parent.clone();
    }
    path.reverse();
    out.extend(path);
    Ok(out)
}
