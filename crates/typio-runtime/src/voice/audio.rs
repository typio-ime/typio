//! Voice audio buffering and post-processing.

/// Initial ring-buffer capacity in samples (30 s at 16 kHz).
pub const INITIAL_BUFFER_SAMPLES: usize = 16000 * 30; // 30 seconds at 16kHz
/// Target sample rate in Hz.
pub const SAMPLE_RATE: usize = 16000;
/// Hard recording bound. Keeps memory and the v1 hex-encoded worker request
/// below the engine protocol's 8 MiB payload ceiling.
pub const MAX_BUFFER_SAMPLES: usize = SAMPLE_RATE * 60;
/// Amplitude threshold below which audio is considered silence.
pub const TRIM_THRESHOLD: f32 = 0.003f32;
/// Padding kept around active audio when trimming silence.
pub const TRIM_PADDING_SAMPLES: usize = SAMPLE_RATE / 10;
/// Minimum number of active samples required to keep audio.
pub const MIN_ACTIVE_SAMPLES: usize = SAMPLE_RATE / 5;

/// Trim leading/trailing silence and return the cleaned audio.
pub fn prepare_audio(audio: &mut Vec<f32>) -> Vec<f32> {
    if audio.is_empty() {
        return Vec::new();
    }
    let peak = audio.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
    let abs_sum: f64 = audio.iter().map(|v| v.abs() as f64).sum();
    let mean_abs = abs_sum / audio.len() as f64;
    log::info!(
        "Voice audio level: duration={:.2}s peak={:.5} mean_abs={:.5}",
        audio.len() as f64 / SAMPLE_RATE as f64,
        peak,
        mean_abs,
    );

    let mut first_active = audio.len();
    let mut last_active = 0usize;
    for (i, &sample) in audio.iter().enumerate() {
        if sample.abs() >= TRIM_THRESHOLD {
            if first_active == audio.len() {
                first_active = i;
            }
            last_active = i;
        }
    }

    if first_active == audio.len()
        || last_active <= first_active
        || last_active - first_active + 1 < MIN_ACTIVE_SAMPLES
    {
        log::warn!("Voice audio discarded: no usable microphone signal detected");
        return Vec::new();
    }

    let start = first_active.saturating_sub(TRIM_PADDING_SAMPLES);
    let end = (last_active + TRIM_PADDING_SAMPLES + 1).min(audio.len());
    if start > 0 || end < audio.len() {
        log::info!(
            "Voice audio trimmed: {:.2}s -> {:.2}s",
            audio.len() as f64 / SAMPLE_RATE as f64,
            (end - start) as f64 / SAMPLE_RATE as f64,
        );
        audio[start..end].to_vec()
    } else {
        std::mem::take(audio)
    }
}
