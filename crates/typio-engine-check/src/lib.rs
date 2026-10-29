//! Black-box conformance checks for isolated Typio engine processes.
//!
//! `typio-engine-check` reads the same manifest as the host, starts the declared
//! executable with a private Engine Protocol channel on fd 3, and validates
//! framing, handshake identity, schema ownership, lifecycle, and basic
//! modality behavior. It never loads engine code into the check process.

use std::path::{Path, PathBuf};

use typio_engine_manifest::{ENGINE_PROTOCOL, EngineManifest};
use typio_engine_protocol::{
    Availability, EngineKind, KeyEvent, KeyResult, KeyState, Reply, ReplyRecord, Request,
};

pub mod check;
pub mod resource;
mod session;

pub use check::{CheckCategory, CheckResult, CheckStatus, Summary};
pub use typio_engine_manifest::{ManifestError, resolve_path_arg};

use session::EngineSession;

const PROTOCOL: CheckCategory = CheckCategory::Protocol;
const BEHAVIOR: CheckCategory = CheckCategory::Behavior;

/// Result of vetting one manifest-declared engine process.
#[derive(Debug)]
pub struct VetReport {
    /// Manifest path supplied by the caller.
    pub manifest_path: PathBuf,
    /// Parsed process manifest.
    pub manifest: EngineManifest,
    /// Ordered conformance results.
    pub results: Vec<CheckResult>,
}

impl VetReport {
    /// Aggregate result counts.
    pub fn summary(&self) -> Summary {
        Summary::of(&self.results)
    }
}

/// Read a manifest and vet its process using an optional package-root override.
pub fn vet_manifest(
    manifest_path: &Path,
    package_root: Option<&Path>,
) -> Result<VetReport, ManifestError> {
    let manifest = EngineManifest::read_from(manifest_path)?;
    let mut results = Vec::new();

    let required_fields_valid = manifest.has_required_fields();
    results.push(if required_fields_valid {
        CheckResult::pass(PROTOCOL, "manifest_fields")
    } else {
        CheckResult::fail(
            PROTOCOL,
            "manifest_fields",
            "name, type, protocol, and command must all be non-empty",
        )
    });

    let protocol_valid = manifest.protocol == ENGINE_PROTOCOL;
    results.push(if protocol_valid {
        CheckResult::pass(PROTOCOL, "manifest_protocol")
    } else {
        CheckResult::fail(
            PROTOCOL,
            "manifest_protocol",
            format!("expected '{ENGINE_PROTOCOL}', got '{}'", manifest.protocol),
        )
    });

    let expected_kind = match manifest.engine_type.as_str() {
        "keyboard" => Some(EngineKind::Keyboard),
        "voice" => Some(EngineKind::Voice),
        other => {
            results.push(CheckResult::fail(
                PROTOCOL,
                "manifest_type",
                format!("unsupported engine type '{other}'"),
            ));
            None
        }
    };
    if expected_kind.is_some() {
        results.push(CheckResult::pass(PROTOCOL, "manifest_type"));
    }

    if required_fields_valid
        && protocol_valid
        && let Some(kind) = expected_kind
    {
        match manifest.argv(manifest_path) {
            Ok(argv) => vet_process(&manifest, kind, &argv, &mut results),
            Err(error) => results.push(CheckResult::fail(
                PROTOCOL,
                "process_spawn",
                error.to_string(),
            )),
        }
    }

    let package = package_root
        .map(Path::to_path_buf)
        .or_else(|| resource::discover_package(manifest_path));
    results.extend(resource::resource_checks(
        manifest.icon.as_deref(),
        package.as_deref(),
    ));

    Ok(VetReport {
        manifest_path: manifest_path.to_path_buf(),
        manifest,
        results,
    })
}

fn vet_process(
    manifest: &EngineManifest,
    expected_kind: EngineKind,
    argv: &[String],
    results: &mut Vec<CheckResult>,
) {
    let (mut session, hello) = match EngineSession::spawn(argv) {
        Ok(value) => {
            results.push(CheckResult::pass(PROTOCOL, "process_spawn"));
            results.push(CheckResult::pass(PROTOCOL, "engine_hello"));
            value
        }
        Err(error) => {
            results.push(CheckResult::fail(PROTOCOL, "process_spawn", error));
            return;
        }
    };

    results.push(if hello.engine == manifest.name {
        CheckResult::pass(PROTOCOL, "engine_identity")
    } else {
        CheckResult::fail(
            PROTOCOL,
            "engine_identity",
            format!(
                "manifest engine '{}' received HELLO for '{}'",
                manifest.name, hello.engine
            ),
        )
    });
    results.push(if hello.kind == expected_kind {
        CheckResult::pass(PROTOCOL, "engine_type")
    } else {
        CheckResult::fail(
            PROTOCOL,
            "engine_type",
            format!(
                "manifest type '{}' received HELLO type '{}'",
                expected_kind.as_str(),
                hello.kind.as_str()
            ),
        )
    });

    let namespace = format!("engines.{}.", manifest.name);
    let invalid_keys = hello
        .schema
        .iter()
        .filter(|field| !field.key.starts_with(&namespace))
        .map(|field| field.key.as_str())
        .collect::<Vec<_>>();
    results.push(if invalid_keys.is_empty() {
        CheckResult::pass(PROTOCOL, "schema_namespace")
    } else {
        CheckResult::fail(
            PROTOCOL,
            "schema_namespace",
            format!(
                "schema keys outside '{namespace}': {}",
                invalid_keys.join(", ")
            ),
        )
    });

    if let Err(error) = session.send_host_hello(&manifest.name, expected_kind) {
        results.push(CheckResult::fail(PROTOCOL, "host_hello", error));
        return;
    }
    results.push(CheckResult::pass(PROTOCOL, "host_hello"));

    let initialized = check_request(
        &mut session,
        Request::Initialize,
        "initialize",
        BEHAVIOR,
        results,
        |_| Ok(()),
    );
    if !initialized {
        let _ = session.stop();
        return;
    }

    check_request(
        &mut session,
        Request::Availability,
        "availability",
        BEHAVIOR,
        results,
        |reply| {
            let state = reply.records.iter().find_map(|record| match record {
                ReplyRecord::Availability(state) => Some(*state),
                _ => None,
            });
            match state {
                Some(Availability::Ready) => Ok(()),
                Some(other) => Err(format!("engine reported {other:?} after initialization")),
                None => Err("response contains no AVAILABILITY record".to_string()),
            }
        },
    );

    match expected_kind {
        EngineKind::Keyboard => vet_keyboard(&mut session, results),
        EngineKind::Voice => vet_voice(&mut session, manifest, results),
    }

    match session.stop() {
        Ok(status) if status.success() => {
            results.push(CheckResult::pass(BEHAVIOR, "clean_shutdown"));
        }
        Ok(status) => results.push(CheckResult::fail(
            BEHAVIOR,
            "clean_shutdown",
            format!("engine exited with {status}"),
        )),
        Err(error) => results.push(CheckResult::fail(BEHAVIOR, "clean_shutdown", error)),
    }
}

fn vet_keyboard(session: &mut EngineSession, results: &mut Vec<CheckResult>) {
    if !check_request(
        session,
        Request::FocusIn(1),
        "focus_in",
        BEHAVIOR,
        results,
        |_| Ok(()),
    ) {
        return;
    }

    let request = Request::ProcessKey(KeyEvent {
        context_id: 1,
        state: KeyState::Press,
        keycode: 30,
        keysym: u32::from(b'a'),
        modifiers: 0,
        unicode: u32::from(b'a'),
        time: 1,
        is_repeat: false,
        base_keysym: u32::from(b'a'),
    });
    match session.request(&request) {
        Ok(reply) => {
            if let Some(error) = reply_error(&reply) {
                results.push(CheckResult::fail(BEHAVIOR, "process_plain_key", error));
            } else {
                match reply.records.iter().find_map(|record| match record {
                    ReplyRecord::KeyResult(result) => Some(*result),
                    _ => None,
                }) {
                    Some(KeyResult::NotHandled) => results.push(CheckResult::warn(
                        BEHAVIOR,
                        "process_plain_key",
                        "engine legally declined a plain ASCII key",
                    )),
                    Some(_) => results.push(CheckResult::pass(BEHAVIOR, "process_plain_key")),
                    None => results.push(CheckResult::fail(
                        BEHAVIOR,
                        "process_plain_key",
                        "response contains no RESULT record",
                    )),
                }
            }
        }
        Err(error) => results.push(CheckResult::fail(BEHAVIOR, "process_plain_key", error)),
    }

    let _ = session.request(&Request::Reset(1));
    let _ = session.request(&Request::FocusOut(1));
}

/// Deterministic non-silent probe audio: 1.5 s of 440 Hz at peak 0.6, 16 kHz
/// mono f32 little-endian.
///
/// A generated waveform is used rather than a bundled fixture because its only
/// job is to prove the engine consumed non-empty input. It cannot go stale and
/// asserts nothing about any model's expected transcription.
fn probe_audio() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 16_000;
    const MILLIS: u32 = 1_500;
    const FREQ_HZ: f32 = 440.0;
    const PEAK: f32 = 0.6;

    let samples = SAMPLE_RATE * MILLIS / 1000;
    let mut bytes = Vec::with_capacity(samples as usize * 4);
    for i in 0..samples {
        let t = i as f32 / SAMPLE_RATE as f32;
        let value = PEAK * (std::f32::consts::TAU * FREQ_HZ * t).sin();
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Probe voice inference with real audio (ADR-0055).
///
/// The previous probe sent an empty payload and accepted any reply containing a
/// `TEXT` record — so a stub returning `Text("")` for every input passed every
/// assertion. The assertion is now on substance:
///
/// * An engine that declares `voice_input` must return **non-empty** text for
///   non-silent audio. Silence, `Text("")`, or a bare `Ok` is a failure.
/// * An engine that does not declare it is judged only on honesty: it must either
///   return non-empty text or an explicit error. A silent empty success is still a
///   failure, because it is indistinguishable from an unimplemented engine.
fn vet_voice(
    session: &mut EngineSession,
    manifest: &EngineManifest,
    results: &mut Vec<CheckResult>,
) {
    let declares_voice = manifest
        .required
        .iter()
        .flatten()
        .chain(manifest.optional.iter().flatten())
        .any(|capability| capability == "voice_input");

    let reply = match session.request(&Request::ProcessAudio(probe_audio())) {
        Ok(reply) => reply,
        Err(error) => {
            results.push(CheckResult::fail(BEHAVIOR, "process_audio", error));
            return;
        }
    };

    if let Some(error) = reply_error(&reply) {
        // An explicit error is an honest answer for an engine that does not
        // claim voice support; it is a broken promise for one that does.
        if declares_voice {
            results.push(CheckResult::fail(
                BEHAVIOR,
                "process_audio",
                format!("declared voice_input but failed with: {error}"),
            ));
        } else {
            results.push(CheckResult::pass(BEHAVIOR, "process_audio"));
        }
        return;
    }

    let text = reply.records.iter().find_map(|record| match record {
        ReplyRecord::Text(text) => Some(text.as_str()),
        _ => None,
    });

    match text {
        Some(transcript) if !transcript.trim().is_empty() => {
            results.push(CheckResult::pass(BEHAVIOR, "process_audio"));
        }
        Some(_) => results.push(CheckResult::fail(
            BEHAVIOR,
            "process_audio",
            "returned an empty transcript for non-silent audio; a stub that never \
             transcribes is indistinguishable from an unimplemented engine",
        )),
        None => results.push(CheckResult::fail(
            BEHAVIOR,
            "process_audio",
            "response contains no TEXT record for non-silent audio",
        )),
    }
}

fn check_request<F>(
    session: &mut EngineSession,
    request: Request,
    name: &'static str,
    category: CheckCategory,
    results: &mut Vec<CheckResult>,
    validate: F,
) -> bool
where
    F: FnOnce(&Reply) -> Result<(), String>,
{
    match session.request(&request) {
        Ok(reply) => {
            let outcome = reply_error(&reply).map_or_else(|| validate(&reply), Err);
            match outcome {
                Ok(()) => {
                    results.push(CheckResult::pass(category, name));
                    true
                }
                Err(error) => {
                    results.push(CheckResult::fail(category, name, error));
                    false
                }
            }
        }
        Err(error) => {
            results.push(CheckResult::fail(category, name, error));
            false
        }
    }
}

fn reply_error(reply: &Reply) -> Option<String> {
    reply.records.iter().find_map(|record| match record {
        ReplyRecord::Error(error) => Some(error.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_required_fields_are_reported_without_spawning() {
        let temp = std::env::temp_dir().join(format!(
            "typio-engine-check-manifest-test-{}-{}.toml",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        std::fs::write(
            &temp,
            "name = \"broken\"\ntype = \"keyboard\"\nprotocol = \"old\"\n",
        )
        .unwrap();
        let report = vet_manifest(&temp, None).unwrap();
        let _ = std::fs::remove_file(&temp);
        assert!(report.results.iter().any(|result| {
            result.name == "manifest_fields" && result.status == CheckStatus::Fail
        }));
        assert!(report.results.iter().any(|result| {
            result.name == "manifest_protocol" && result.status == CheckStatus::Fail
        }));
    }
}
