//! Checked feature claims: every native pass must be recomputable from hashed
//! expected and actual traces. A shader probe cannot promote a game scenario.
use crate::parity::{self, Origin, ParityReport, ScenarioTrace};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    NotStarted,
    Decoded,
    Implemented,
    NativeComparisonPassed,
    IntegratedScenarioPassed,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Infrastructure,
    ShaderProgram,
    GameScenario,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEvidence {
    pub path: PathBuf,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeEvidence {
    pub report: FileEvidence,
    pub expected: FileEvidence,
    pub actual: FileEvidence,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub id: String,
    pub scope: Scope,
    pub state: State,
    pub notes: String,
    pub native_comparisons: Vec<NativeEvidence>,
    pub repeatability: Vec<FileEvidence>,
    pub observer_baseline: Option<NativeEvidence>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub version: u32,
    pub features: Vec<Feature>,
}
fn load<T: serde::de::DeserializeOwned>(base: &Path, evidence: &FileEvidence) -> Result<T> {
    let path = base.join(&evidence.path);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !parity::sha256(&bytes).eq_ignore_ascii_case(&evidence.sha256) {
        return Err(format!("ledger evidence changed: {}", path.display()));
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}
fn verify_comparison(
    base: &Path,
    evidence: &NativeEvidence,
) -> Result<(ScenarioTrace, ScenarioTrace)> {
    let expected: ScenarioTrace = load(base, &evidence.expected)?;
    let actual: ScenarioTrace = load(base, &evidence.actual)?;
    let recorded: ParityReport = load(base, &evidence.report)?;
    let computed = parity::compare(&expected, &actual)?;
    if !computed.passed
        || computed.compared_frames == 0
        || computed.compared_pixels == 0
        || serde_json::to_value(&computed).map_err(|e| e.to_string())?
            != serde_json::to_value(&recorded).map_err(|e| e.to_string())?
    {
        return Err("native evidence is not an exact, nonempty, reproducible comparison".into());
    }
    Ok((expected, actual))
}
fn complete_scene(trace: &ScenarioTrace) -> bool {
    trace.frames.iter().all(|f| {
        !f.inputs.is_null()
            && !f.rng.is_null()
            && !f.state.is_null()
            && !f.animation.is_null()
            && !f.audio.is_null()
            && f.images.contains_key("final")
    })
}
impl Ledger {
    pub fn validate(&self, base: &Path) -> Result<()> {
        if self.version != 1 || self.features.is_empty() {
            return Err("empty/unsupported parity ledger".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for feature in &self.features {
            if feature.id.is_empty() || feature.notes.is_empty() || !ids.insert(&feature.id) {
                return Err("invalid or duplicate ledger feature".into());
            }
            let native = matches!(
                feature.state,
                State::NativeComparisonPassed | State::IntegratedScenarioPassed
            );
            if !native {
                continue;
            }
            if feature.native_comparisons.is_empty() || feature.scope == Scope::Infrastructure {
                return Err(format!(
                    "{}: native claim lacks comparison evidence",
                    feature.id
                ));
            }
            for evidence in &feature.native_comparisons {
                let (expected, actual) = verify_comparison(base, evidence)?;
                match feature.scope {
                    Scope::ShaderProgram
                        if expected.origin == Origin::ShaderProbe
                            && actual.origin == Origin::ShaderProbe => {}
                    Scope::GameScenario
                        if expected.origin == Origin::OriginalGame
                            && actual.origin == Origin::Rewrite
                            && complete_scene(&expected)
                            && complete_scene(&actual) => {}
                    _ => {
                        return Err(format!(
                            "{}: trace origin or coverage does not support feature scope",
                            feature.id
                        ))
                    }
                }
            }
            if feature.scope == Scope::GameScenario {
                let traces = feature
                    .repeatability
                    .iter()
                    .map(|e| load(base, e))
                    .collect::<Result<Vec<ScenarioTrace>>>()?;
                parity::repeatability(&traces)?;
                let (baseline, observed) = verify_comparison(
                    base,
                    feature
                        .observer_baseline
                        .as_ref()
                        .ok_or("original scene lacks ordinary-vs-observed comparison")?,
                )?;
                if baseline.origin != Origin::OriginalGame
                    || observed.origin != Origin::OriginalGame
                    || baseline.scenario != traces[0].scenario
                    || baseline.manifest_sha256 != traces[0].manifest_sha256
                {
                    return Err(
                        "observer effect comparison does not match the native scenario".into(),
                    );
                }
                for evidence in &feature.native_comparisons {
                    let (expected, _) = verify_comparison(base, evidence)?;
                    if !parity::compare(&traces[0], &expected)?.passed {
                        return Err("native comparison uses a different original scenario".into());
                    }
                }
            }
            if feature.state == State::IntegratedScenarioPassed
                && feature.scope != Scope::GameScenario
            {
                return Err("shader probes cannot establish integrated game parity".into());
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoded_or_implemented_features_never_count_as_native() {
        let mut ledger = Ledger {
            version: 1,
            features: vec![Feature {
                id: "native-world".into(),
                scope: Scope::GameScenario,
                state: State::Implemented,
                notes: "World implementation, no native capture yet".into(),
                native_comparisons: vec![],
                repeatability: vec![],
                observer_baseline: None,
            }],
        };
        assert!(ledger.validate(Path::new(".")).is_ok());
        ledger.features[0].state = State::NativeComparisonPassed;
        assert!(ledger.validate(Path::new(".")).is_err());
        ledger.features[0].state = State::IntegratedScenarioPassed;
        assert!(ledger.validate(Path::new(".")).is_err());
        ledger.features[0].state = State::Implemented;
        ledger.features.push(ledger.features[0].clone());
        assert!(ledger.validate(Path::new(".")).is_err());
    }
}
