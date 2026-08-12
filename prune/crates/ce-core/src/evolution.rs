use crate::model::StrategyConfig;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectiveVector {
    pub resolved_rate: f64,
    pub tokens: f64,
    pub latency_ms: f64,
    pub redundancy_pct: f64,
    pub missing_definition_risk: f64,
}

pub fn dominates(a: ObjectiveVector, b: ObjectiveVector) -> bool {
    let no_worse = a.resolved_rate >= b.resolved_rate
        && a.tokens <= b.tokens
        && a.latency_ms <= b.latency_ms
        && a.redundancy_pct <= b.redundancy_pct
        && a.missing_definition_risk <= b.missing_definition_risk;
    let strictly_better = a.resolved_rate > b.resolved_rate
        || a.tokens < b.tokens
        || a.latency_ms < b.latency_ms
        || a.redundancy_pct < b.redundancy_pct
        || a.missing_definition_risk < b.missing_definition_risk;
    no_worse && strictly_better
}

pub fn pareto_front(objectives: &[ObjectiveVector]) -> Vec<usize> {
    (0..objectives.len())
        .filter(|&i| {
            !(0..objectives.len()).any(|j| i != j && dominates(objectives[j], objectives[i]))
        })
        .collect()
}

/// Uniform crossover over serialized strategy fields. Nested policy fields are
/// crossed independently, while deserialization supplies compatibility checks.
pub fn crossover_strategy(a: &StrategyConfig, b: &StrategyConfig, seed: u64) -> StrategyConfig {
    let mut left = serde_json::to_value(a).expect("StrategyConfig serializes");
    let right = serde_json::to_value(b).expect("StrategyConfig serializes");
    cross_value(&mut left, &right, seed, "strategy");
    serde_json::from_value(left).expect("crossover preserves StrategyConfig schema")
}

fn cross_value(left: &mut Value, right: &Value, seed: u64, path: &str) {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            for (key, value) in left.iter_mut() {
                if let Some(other) = right.get(key) {
                    cross_value(value, other, seed, &format!("{path}.{key}"));
                }
            }
        }
        (left, right) => {
            let hash = crate::util::hash_text_hex(&format!("{seed}:{path}"));
            if hash.as_bytes().first().is_some_and(|byte| byte % 2 == 0) {
                *left = right.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pareto_front_excludes_dominated_candidate() {
        let values = vec![
            ObjectiveVector {
                resolved_rate: 1.0,
                tokens: 100.0,
                latency_ms: 10.0,
                redundancy_pct: 0.0,
                missing_definition_risk: 0.0,
            },
            ObjectiveVector {
                resolved_rate: 0.5,
                tokens: 200.0,
                latency_ms: 20.0,
                redundancy_pct: 10.0,
                missing_definition_risk: 1.0,
            },
            ObjectiveVector {
                resolved_rate: 0.9,
                tokens: 50.0,
                latency_ms: 8.0,
                redundancy_pct: 0.0,
                missing_definition_risk: 0.0,
            },
        ];
        assert_eq!(pareto_front(&values), vec![0, 2]);
    }

    #[test]
    fn crossover_is_deterministic_and_uses_both_parents() {
        let mut a = StrategyConfig::default();
        a.lexical_k = 10;
        a.semantic_k = 20;
        let mut b = StrategyConfig::default();
        b.lexical_k = 90;
        b.semantic_k = 80;
        let child = crossover_strategy(&a, &b, 42);
        assert_eq!(child, crossover_strategy(&a, &b, 42));
        assert!([10, 90].contains(&child.lexical_k));
        assert!([20, 80].contains(&child.semantic_k));
    }
}
