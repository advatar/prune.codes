use crate::model::{StrategyConfig, StrategySelection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskClass {
    Bugfix,
    Feature,
    Refactor,
    Integration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryArchetype {
    Rust,
    Web,
    Apple,
    Mixed,
    Unknown,
}

#[derive(Debug, Clone, Default)]
pub struct RepositorySignals {
    pub rust_files: usize,
    pub ts_files: usize,
    pub tsx_files: usize,
    pub swift_files: usize,
}

pub fn select_strategy(
    task: &str,
    repo: &RepositorySignals,
) -> (StrategyConfig, StrategySelection) {
    let task_class = classify_task(task);
    let archetype = classify_repository(repo);
    let mut strategy = StrategyConfig::default();
    match task_class {
        TaskClass::Bugfix => {
            strategy.signals_enabled = true;
            strategy.support_enabled = true;
            strategy.max_bodies = 2;
        }
        TaskClass::Feature => {
            strategy.include_api_summaries = true;
            strategy.max_bodies = 3;
            strategy.edge_radius = 3;
        }
        TaskClass::Refactor => {
            strategy.support_enabled = true;
            strategy.subgraph_enabled = true;
            strategy.per_file_cap_signatures = 12;
        }
        TaskClass::Integration => {
            strategy.include_api_summaries = true;
            strategy.edge_module_radius = 2;
            strategy.edge_reverse_radius = 2;
        }
    }
    match archetype {
        RepositoryArchetype::Web => strategy.ast_slice_policy.top_k_blocks = 10,
        RepositoryArchetype::Apple => strategy.ast_slice_policy.max_depth = 12,
        RepositoryArchetype::Mixed => strategy.candidate_pool_limit = 350,
        RepositoryArchetype::Rust | RepositoryArchetype::Unknown => {}
    }
    let known_files = repo.rust_files + repo.ts_files + repo.tsx_files + repo.swift_files;
    let confidence = if known_files == 0 { 0.35 } else { 0.9 };
    let selection = StrategySelection {
        task_class: format!("{:?}", task_class).to_ascii_lowercase(),
        repository_archetype: format!("{:?}", archetype).to_ascii_lowercase(),
        confidence,
        reason: if known_files == 0 {
            "task-class profile with conservative default archetype fallback".to_string()
        } else {
            "task-class profile adjusted for indexed language distribution".to_string()
        },
    };
    (strategy, selection)
}

fn classify_task(task: &str) -> TaskClass {
    let task = task.to_ascii_lowercase();
    if ["bug", "fix", "error", "crash", "fail"]
        .iter()
        .any(|word| task.contains(word))
    {
        TaskClass::Bugfix
    } else if ["refactor", "rename", "extract", "restructure"]
        .iter()
        .any(|word| task.contains(word))
    {
        TaskClass::Refactor
    } else if ["integrate", "integration", "webhook", "api", "connect"]
        .iter()
        .any(|word| task.contains(word))
    {
        TaskClass::Integration
    } else {
        TaskClass::Feature
    }
}

fn classify_repository(repo: &RepositorySignals) -> RepositoryArchetype {
    let groups = [
        repo.rust_files > 0,
        repo.ts_files + repo.tsx_files > 0,
        repo.swift_files > 0,
    ];
    if groups.iter().filter(|&&present| present).count() > 1 {
        RepositoryArchetype::Mixed
    } else if repo.rust_files > 0 {
        RepositoryArchetype::Rust
    } else if repo.ts_files + repo.tsx_files > 0 {
        RepositoryArchetype::Web
    } else if repo.swift_files > 0 {
        RepositoryArchetype::Apple
    } else {
        RepositoryArchetype::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_explainable_bugfix_profile_for_apple_repo() {
        let (strategy, decision) = select_strategy(
            "Fix crash in ContentView",
            &RepositorySignals {
                swift_files: 20,
                ..Default::default()
            },
        );
        assert!(strategy.support_enabled);
        assert_eq!(decision.task_class, "bugfix");
        assert_eq!(decision.repository_archetype, "apple");
        assert!(decision.confidence > 0.5);
    }

    #[test]
    fn unknown_repo_has_explicit_low_confidence_fallback() {
        let (_, decision) = select_strategy("Add a setting", &RepositorySignals::default());
        assert_eq!(decision.repository_archetype, "unknown");
        assert!(decision.reason.contains("fallback"));
    }
}
