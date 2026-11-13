//! Evidence boundary for parity work. Decoding is never counted as native parity.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}
pub fn fingerprint(path: &Path) -> Result<Fingerprint, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut file = File::open(&path).map_err(|e| e.to_string())?;
    let bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    let mut count = 0;
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        count += n as u64;
    }
    if count != bytes || file.metadata().map_err(|e| e.to_string())?.len() != bytes {
        return Err("reference file changed during hashing".into());
    }
    Ok(Fingerprint {
        path,
        bytes,
        sha256: format!("{:x}", digest.finalize()),
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceManifest {
    pub version: u32,
    pub profile: String,
    pub executable: Fingerprint,
    pub packages: Vec<Fingerprint>,
    pub preferences: Vec<Fingerprint>,
    pub adapter: Value,
    pub locale: String,
    /// Explicit settings, not inferred from the desktop resolution.
    pub settings: Value,
}
impl ReferenceManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.profile.is_empty()
            || self.packages.is_empty()
            || self.locale.is_empty()
            || !self.settings.is_object()
        {
            return Err("incomplete or unsupported reference manifest".into());
        }
        let mut paths = std::collections::BTreeSet::new();
        for f in std::iter::once(&self.executable)
            .chain(&self.packages)
            .chain(&self.preferences)
        {
            if f.bytes == 0
                || f.sha256.len() != 64
                || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || !paths.insert(&f.path)
            {
                return Err("invalid/duplicate manifest fingerprint".into());
            }
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<String, String> {
        self.validate()?;
        // Serialization order is fixed by structs/BTreeMap, and the full adapter,
        // executable, settings, package order, and preference hashes participate.
        Ok(sha256(
            &serde_json::to_vec(self).map_err(|e| e.to_string())?,
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileIsolation {
    pub private_root: PathBuf,
    pub app_data: PathBuf,
    pub creations: PathBuf,
    /// Observer must prove these were selected before the first resource/profile access.
    pub redirect_sequence: u64,
    pub first_profile_access_sequence: u64,
    pub observed_reads: Vec<PathBuf>,
    pub observed_writes: Vec<PathBuf>,
    pub observer_sha256: String,
}
impl ProfileIsolation {
    pub fn validate(&self) -> Result<(), String> {
        let root = self
            .private_root
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let allowed = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/reference-profiles")
            .canonicalize()
            .map_err(|_| "private reference profile parent has not been created")?;
        if root == allowed || !root.starts_with(&allowed) {
            return Err(
                "reference profiles must be isolated below ignored target/reference-profiles"
                    .into(),
            );
        }
        if self.redirect_sequence >= self.first_profile_access_sequence
            || self.observed_reads.is_empty()
            || self.observer_sha256.len() != 64
            || !self.observer_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("early profile isolation not evidenced".into());
        }
        let data = self.app_data.canonicalize().map_err(|e| e.to_string())?;
        let creations = self.creations.canonicalize().map_err(|e| e.to_string())?;
        if data == creations {
            return Err("profile and creations directories must differ".into());
        }
        for path in std::iter::once(&data)
            .chain(std::iter::once(&creations))
            .chain(&self.observed_reads)
            .chain(&self.observed_writes)
        {
            // Require existing, resolved paths. A receipt must not hide an escaping
            // junction, nonexistent path or traversal behind a textual prefix.
            let path = path.canonicalize().map_err(|e| e.to_string())?;
            if path == root || !path.starts_with(&root) {
                return Err(format!(
                    "profile access escapes private root: {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Origin {
    OriginalGame,
    Rewrite,
    ShaderProbe,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioTrace {
    pub version: u32,
    pub manifest_sha256: String,
    pub scenario: String,
    pub origin: Origin,
    pub isolation: Option<ProfileIsolation>,
    pub frames: Vec<Frame>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub tick: u64,
    pub delta_bits: u32,
    pub inputs: Value,
    pub rng: Value,
    pub state: Value,
    pub animation: Value,
    pub audio: Value,
    /// Each intermediate target, UI layer, or final frame is independently compared.
    pub images: BTreeMap<String, Image>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}
impl ScenarioTrace {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.scenario.is_empty()
            || self.manifest_sha256.len() != 64
            || !self.manifest_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.frames.is_empty()
        {
            return Err("incomplete scenario trace".into());
        }
        if self.origin == Origin::OriginalGame {
            self.isolation
                .as_ref()
                .ok_or("original capture lacks profile isolation evidence")?
                .validate()?;
        }
        let mut last = None;
        for frame in &self.frames {
            if last.is_some_and(|v| frame.tick <= v)
                || !f32::from_bits(frame.delta_bits).is_finite()
                || f32::from_bits(frame.delta_bits) < 0.0
            {
                return Err("invalid frame order or delta".into());
            }
            last = Some(frame.tick);
            for image in frame.images.values() {
                if image.width == 0
                    || image.height == 0
                    || image.width > 8192
                    || image.height > 8192
                    || image.rgba8.len() as u64 != image.width as u64 * image.height as u64 * 4
                {
                    return Err("invalid capture image".into());
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Divergence {
    pub tick: Option<u64>,
    pub path: String,
    pub expected: String,
    pub actual: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParityReport {
    pub version: u32,
    pub scenario: String,
    pub expected_trace_sha256: String,
    pub actual_trace_sha256: String,
    pub compared_frames: usize,
    pub compared_pixels: u64,
    pub passed: bool,
    pub first_divergence: Option<Divergence>,
}
fn diff_value(a: &Value, b: &Value, path: &str) -> Option<(String, String, String)> {
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).collect();
            for k in keys {
                let p = format!("{path}/{}", k.replace('~', "~0").replace('/', "~1"));
                match (a.get(k), b.get(k)) {
                    (Some(a), Some(b)) => {
                        if let Some(d) = diff_value(a, b, &p) {
                            return Some(d);
                        }
                    }
                    (a, b) => {
                        return Some((
                            p,
                            a.map_or("<missing>".into(), Value::to_string),
                            b.map_or("<missing>".into(), Value::to_string),
                        ))
                    }
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) => {
            for i in 0..a.len().min(b.len()) {
                if let Some(d) = diff_value(&a[i], &b[i], &format!("{path}/{i}")) {
                    return Some(d);
                }
            }
            if a.len() != b.len() {
                Some((
                    format!("{path}/length"),
                    a.len().to_string(),
                    b.len().to_string(),
                ))
            } else {
                None
            }
        }
        _ if a != b => Some((path.into(), a.to_string(), b.to_string())),
        _ => None,
    }
}
pub fn compare(expected: &ScenarioTrace, actual: &ScenarioTrace) -> Result<ParityReport, String> {
    expected.validate()?;
    actual.validate()?;
    let mut report = ParityReport {
        version: 1,
        scenario: expected.scenario.clone(),
        expected_trace_sha256: sha256(&serde_json::to_vec(expected).map_err(|e| e.to_string())?),
        actual_trace_sha256: sha256(&serde_json::to_vec(actual).map_err(|e| e.to_string())?),
        compared_frames: 0,
        compared_pixels: 0,
        passed: false,
        first_divergence: None,
    };
    let mut divergence = |tick, path: String, a: String, b: String| {
        report.first_divergence = Some(Divergence {
            tick,
            path,
            expected: a,
            actual: b,
        });
    };
    for (path, a, b) in [
        (
            "manifest_sha256",
            &expected.manifest_sha256,
            &actual.manifest_sha256,
        ),
        ("scenario", &expected.scenario, &actual.scenario),
    ] {
        if a != b {
            divergence(None, path.into(), a.clone(), b.clone());
            return Ok(report);
        }
    }
    for (a, b) in expected.frames.iter().zip(&actual.frames) {
        let tick = Some(a.tick);
        for (path, av, bv) in [
            ("tick", a.tick, b.tick),
            ("delta_bits", a.delta_bits as u64, b.delta_bits as u64),
        ] {
            if av != bv {
                report.first_divergence = Some(Divergence {
                    tick,
                    path: path.into(),
                    expected: av.to_string(),
                    actual: bv.to_string(),
                });
                return Ok(report);
            }
        }
        for (path, av, bv) in [
            ("inputs", &a.inputs, &b.inputs),
            ("rng", &a.rng, &b.rng),
            ("state", &a.state, &b.state),
            ("animation", &a.animation, &b.animation),
            ("audio", &a.audio, &b.audio),
        ] {
            if let Some((path, expected, actual)) = diff_value(av, bv, path) {
                report.first_divergence = Some(Divergence {
                    tick,
                    path,
                    expected,
                    actual,
                });
                return Ok(report);
            }
        }
        if a.images.keys().ne(b.images.keys()) {
            report.first_divergence = Some(Divergence {
                tick,
                path: "images/targets".into(),
                expected: format!("{:?}", a.images.keys()),
                actual: format!("{:?}", b.images.keys()),
            });
            return Ok(report);
        }
        for (name, av) in &a.images {
            let bv = &b.images[name];
            if (av.width, av.height) != (bv.width, bv.height) {
                report.first_divergence = Some(Divergence {
                    tick,
                    path: format!("images/{name}/dimensions"),
                    expected: format!("{}x{}", av.width, av.height),
                    actual: format!("{}x{}", bv.width, bv.height),
                });
                return Ok(report);
            }
            for (i, (ap, bp)) in av
                .rgba8
                .chunks_exact(4)
                .zip(bv.rgba8.chunks_exact(4))
                .enumerate()
            {
                if ap != bp {
                    report.first_divergence = Some(Divergence {
                        tick,
                        path: format!(
                            "images/{name}/pixel/{},{}",
                            i % av.width as usize,
                            i / av.width as usize
                        ),
                        expected: format!("{ap:?}"),
                        actual: format!("{bp:?}"),
                    });
                    return Ok(report);
                }
                report.compared_pixels += 1;
            }
        }
        report.compared_frames += 1;
    }
    if expected.frames.len() != actual.frames.len() {
        report.first_divergence = Some(Divergence {
            tick: None,
            path: "frames/length".into(),
            expected: expected.frames.len().to_string(),
            actual: actual.frames.len().to_string(),
        });
    }
    report.passed = report.first_divergence.is_none();
    Ok(report)
}
/// Three equal original captures establish a repeatable scenario. No broad SSIM
/// threshold, pixel mask, or tolerance can conceal an unsupported render path.
pub fn repeatability(traces: &[ScenarioTrace]) -> Result<Vec<ParityReport>, String> {
    if traces.len() != 3 || traces.iter().any(|t| t.origin != Origin::OriginalGame) {
        return Err("repeatability requires three original-game captures".into());
    }
    let reports = vec![
        compare(&traces[0], &traces[1])?,
        compare(&traces[0], &traces[2])?,
    ];
    if reports.iter().any(|r| !r.passed) {
        return Err("original scenario is not repeatable; inspect pairwise comparisons".into());
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn trace() -> ScenarioTrace {
        ScenarioTrace {
            version: 1,
            manifest_sha256: "a".repeat(64),
            scenario: "synthetic-check".into(),
            origin: Origin::ShaderProbe,
            isolation: None,
            frames: vec![Frame {
                tick: 10,
                delta_bits: (1.0f32 / 60.0).to_bits(),
                inputs: Value::Null,
                rng: serde_json::json!([1, 2]),
                state: serde_json::json!({"actors":[{"health":10}]}),
                animation: Value::Null,
                audio: Value::Null,
                images: BTreeMap::from([(
                    "final".into(),
                    Image {
                        width: 2,
                        height: 1,
                        rgba8: vec![0; 8],
                    },
                )]),
            }],
        }
    }
    #[test]
    fn identifies_first_state_divergence() {
        let a = trace();
        let mut b = a.clone();
        b.frames[0].state["actors"][0]["health"] = 9.into();
        let r = compare(&a, &b).unwrap();
        assert!(!r.passed);
        let d = r.first_divergence.unwrap();
        assert_eq!(d.tick, Some(10));
        assert_eq!(d.path, "state/actors/0/health");
    }
    #[test]
    fn detects_single_pixel_and_missing_frame() {
        let a = trace();
        assert!(compare(&a, &a).unwrap().passed);
        let mut b = a.clone();
        b.frames[0].images.get_mut("final").unwrap().rgba8[7] = 1;
        assert_eq!(
            compare(&a, &b).unwrap().first_divergence.unwrap().path,
            "images/final/pixel/1,0"
        );
        b = a.clone();
        let mut frame = b.frames[0].clone();
        frame.tick = 11;
        b.frames.push(frame);
        assert_eq!(
            compare(&a, &b).unwrap().first_divergence.unwrap().path,
            "frames/length"
        );
    }
    #[test]
    fn refuses_false_native_evidence_and_bad_schema() {
        let mut a = trace();
        assert!(repeatability(&[a.clone(), a.clone(), a.clone()]).is_err());
        a.origin = Origin::OriginalGame;
        assert!(a.validate().is_err());
        a = trace();
        a.frames[0].images.get_mut("final").unwrap().rgba8.pop();
        assert!(a.validate().is_err());
        a = trace();
        a.version = 2;
        assert!(a.validate().is_err());
        a = trace();
        a.frames[0].delta_bits = f32::NAN.to_bits();
        assert!(a.validate().is_err());
    }
    #[test]
    fn identity_and_inputs_are_strict() {
        let a = trace();
        let mut b = a.clone();
        b.manifest_sha256 = "b".repeat(64);
        assert_eq!(
            compare(&a, &b).unwrap().first_divergence.unwrap().path,
            "manifest_sha256"
        );
        b = a.clone();
        b.frames[0].inputs = serde_json::json!({"key":32});
        assert_eq!(
            compare(&a, &b).unwrap().first_divergence.unwrap().path,
            "inputs"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
