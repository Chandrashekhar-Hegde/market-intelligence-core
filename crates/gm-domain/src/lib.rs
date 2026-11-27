pub mod analysis;
pub mod context;
pub mod event_study;
pub mod features;
pub mod indicators;
pub mod model;
pub mod ontology;
pub mod quality;
pub mod risk;
pub mod rules;
pub mod scoring;
pub mod stochastic;
pub mod taxonomy;
pub mod types;

pub use context::{MacroContext, MacroInputs, build_macro_context};
pub use features::{FeatureVector, compute_features};
pub use model::{
    CandidateAction, DECISION_MODEL_VERSION, EventStudyEvidence, ModelReport, build_model_report,
    input_hash,
};
pub use ontology::{
    EntityType, KnowledgeEntity, KnowledgeRelationship, OntologyFixture, Provenance,
    RelationshipType, mv_entity_types, mv_event_classes, mv_fixture_graph, mv_relationship_types,
};
pub use rules::{RuleDefinition, RuleRegistry, RuleResult};
pub use scoring::{AsOfFacts, DecisionInput, DecisionThresholds, decide, score_event};
pub use stochastic::{PredictionRecord, gbm_flow_prediction};
pub use taxonomy::{EventClass, classify};
pub use types::{Action, Decision, NormalizedEvent, PriceBar};

#[cfg(test)]
mod register {
    /// Every `const` in this crate is listed in docs/CONSTANTS.md with its kind and source.
    #[test]
    fn every_constant_is_registered() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let register = std::fs::read_to_string(root.join("../../docs/CONSTANTS.md")).unwrap();
        let mut missing = Vec::new();
        let mut found = 0;
        let mut stack = vec![root.join("src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("tests.rs")
                {
                    for name in const_names(&std::fs::read_to_string(&path).unwrap()) {
                        found += 1;
                        if !register.contains(&format!("`{name}`")) {
                            missing.push(format!("{} in {}", name, path.display()));
                        }
                    }
                }
            }
        }
        // Word lists are the reader's vocabulary, documented in METHODOLOGY, not numbers.
        missing.retain(|m| {
            ![
                "UP ",
                "DOWN ",
                "RELIEF ",
                "POSITIVE_WORDS ",
                "NEGATIVE_WORDS ",
                "RULES ",
                "RATE_UP ",
                "RATE_DOWN ",
                "FLOW_UP ",
                "FLOW_DOWN ",
                "MORE_UP ",
                "MORE_DOWN ",
                "RATE_CONTEXT ",
                "RATE_DOUBT ",
                "CLAUSE_BREAKS ",
                "REPORTS ",
                "AI ",
                "THEMES ",
                "ASSETS ",
            ]
            .iter()
            .any(|w| m.starts_with(w))
        });
        assert!(found > 60, "the scan found only {found} constants");
        assert!(missing.is_empty(), "add to docs/CONSTANTS.md: {missing:#?}");
    }

    /// Names declared as `const NAME:` (no regex dependency).
    fn const_names(src: &str) -> Vec<String> {
        src.lines()
            .filter_map(|line| {
                let t = line.trim_start();
                let t = t
                    .strip_prefix("pub(super) ")
                    .or_else(|| t.strip_prefix("pub "))
                    .unwrap_or(t);
                let rest = t.strip_prefix("const ")?;
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                (!name.is_empty() && rest[name.len()..].trim_start().starts_with(':'))
                    .then_some(name)
            })
            .collect()
    }
}
