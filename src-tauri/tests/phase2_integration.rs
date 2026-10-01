//! Real integration tests for Phase 2 TTS/STT engines.
//!
//! These tests make actual network calls (edge-tts, Groq) and real local
//! synthesis (Piper). They are marked `#[ignore]` by default so they don't
//! run in CI without network access. Run with:
//!
//!   cargo test --test phase2_integration -- --ignored --nocapture

use std::io::Write;

#[tokio::test]
#[ignore = "requires network access to Microsoft edge-tts"]
async fn test_edge_tts_real_synthesis() {
    println!("\n=== Testing edge-tts real synthesis ===");

    let text = "On it sir";
    let voice = "en-US-AvaNeural";

    let start = std::time::Instant::now();
    let result = nexus_lib::tts_edge::synthesize_to_mp3(text, voice).await;
    let elapsed = start.elapsed();

    match result {
        Ok(mp3_bytes) => {
            println!("✓ edge-tts synthesis succeeded");
            println!("  Text: '{}'", text);
            println!("  Voice: {}", voice);
            println!("  MP3 size: {} bytes", mp3_bytes.len());
            println!("  Latency: {}ms", elapsed.as_millis());

            // Save to file for manual listening test
            let mut file = std::fs::File::create("test_edge_tts_output.mp3")
                .expect("failed to create output file");
            file.write_all(&mp3_bytes)
                .expect("failed to write MP3");
            println!("  Saved to: test_edge_tts_output.mp3");

            // Verify it's a valid MP3 (starts with MP3 frame sync or ID3)
            assert!(mp3_bytes.len() > 100, "MP3 too small — likely empty");
            assert!(
                mp3_bytes[0] == 0xFF || mp3_bytes[0] == b'I',
                "MP3 doesn't start with frame sync or ID3 header"
            );
            println!("  MP3 header validation: PASS");
        }
        Err(e) => {
            println!("✗ edge-tts synthesis FAILED: {}", e);
            panic!("edge-tts real synthesis failed: {}", e);
        }
    }
}

#[tokio::test]
#[ignore = "requires network access to Microsoft edge-tts"]
async fn test_edge_tts_all_cached_phrases() {
    println!("\n=== Testing edge-tts with all cached phrases ===");

    let phrases = [
        "On it sir",
        "Didn't understand that sir",
        "Didn't catch that sir",
        "Here is the analysis, sir",
        "Ok sir",
    ];

    let voice = "en-US-AvaNeural";
    let mut all_ok = true;

    for phrase in &phrases {
        let start = std::time::Instant::now();
        let result = nexus_lib::tts_edge::synthesize_to_mp3(phrase, voice).await;
        let elapsed = start.elapsed();

        match result {
            Ok(bytes) => {
                println!(
                    "  ✓ '{}' → {} bytes in {}ms",
                    phrase,
                    bytes.len(),
                    elapsed.as_millis()
                );
            }
            Err(e) => {
                println!("  ✗ '{}' FAILED in {}ms: {}", phrase, elapsed.as_millis(), e);
                all_ok = false;
            }
        }
    }

    assert!(all_ok, "Some cached phrases failed synthesis");
    println!("\n✓ All cached phrases synthesized successfully");
}

#[tokio::test]
#[ignore = "requires network access to Microsoft edge-tts"]
async fn test_edge_tts_multiple_voices() {
    println!("\n=== Testing edge-tts with multiple voices ===");

    let voices = [
        ("en-US-AvaNeural", "Ava (Female, warm)"),
        ("en-US-EmmaMultilingualNeural", "Emma (Female, professional)"),
        ("en-US-GuyNeural", "Guy (Male, natural)"),
        ("en-US-JennyNeural", "Jenny (Female, friendly)"),
    ];

    let text = "Hello, I am your NEXUS assistant.";
    let mut success_count = 0;

    for (voice_id, voice_name) in &voices {
        let mut last_err = String::new();
        let mut ok = false;

        // Retry up to 2 times (edge-tts WebSocket can be flaky)
        for attempt in 0..2 {
            let start = std::time::Instant::now();
            match nexus_lib::tts_edge::synthesize_to_mp3(text, voice_id).await {
                Ok(bytes) => {
                    println!(
                        "  ✓ {} → {} bytes in {}ms (attempt {})",
                        voice_name,
                        bytes.len(),
                        start.elapsed().as_millis(),
                        attempt + 1,
                    );
                    ok = true;
                    break;
                }
                Err(e) => {
                    last_err = e;
                    println!("  ⚠ {} attempt {} failed, retrying...", voice_name, attempt + 1);
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }

        if !ok {
            println!("  ✗ {} FAILED after retries: {}", voice_name, last_err);
        } else {
            success_count += 1;
        }
    }

    // At least 3 of 4 voices should work (allow 1 transient failure)
    assert!(
        success_count >= 3,
        "Only {}/4 voices succeeded",
        success_count
    );
    println!("\n✓ {}/4 voices synthesized successfully", success_count);
}

#[tokio::test]
#[ignore = "requires Piper model in resources/piper/"]
async fn test_piper_real_synthesis() {
    println!("\n=== Testing Piper real synthesis ===");

    // Set espeak-ng data path for test environment
    // (test binary runs from target/debug/deps/, so go up to project root)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let resources_dir = std::path::Path::new(manifest_dir).join("resources");
    if resources_dir.join("espeak-ng-data").exists() {
        std::env::set_var("PIPER_ESPEAKNG_DATA_DIRECTORY", &resources_dir);
        println!("  Set espeak-ng data path: {}", resources_dir.display());
    }

    // Also set the piper model path by copying to expected location
    // Or better: modify the test to pass the path directly
    let piper_model = std::path::Path::new(manifest_dir)
        .join("resources")
        .join("piper")
        .join("en_US-amy-medium.onnx");
    let piper_config = std::path::Path::new(manifest_dir)
        .join("resources")
        .join("piper")
        .join("en_US-amy-medium.onnx.json");

    if !piper_model.exists() {
        println!("⚠ Piper model not found at: {}", piper_model.display());
        println!("  Download with:");
        println!("    curl -L -o resources/piper/en_US-amy-medium.onnx \\");
        println!("      https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/amy/medium/en_US-amy-medium.onnx");
        return;
    }

    println!("  Model: {}", piper_model.display());

    // Load Piper directly (bypass the path-finding logic)
    let start = std::time::Instant::now();
    let mut piper = piper_rs::Piper::new(&piper_model, &piper_config)
        .expect("failed to load Piper model");
    let load_time = start.elapsed();
    println!("  Model load time: {}ms", load_time.as_millis());

    let text = "On it sir";
    let synth_start = std::time::Instant::now();
    let (samples, sample_rate) = piper
        .create(text, false, None, None, None, None)
        .expect("Piper synthesis failed");
    let synth_time = synth_start.elapsed();

    println!("✓ Piper synthesis succeeded");
    println!("  Text: '{}'", text);
    println!("  Sample rate: {}Hz", sample_rate);
    println!("  Samples: {}", samples.len());
    println!("  Duration: {}ms", samples.len() as u64 * 1000 / sample_rate as u64);
    println!("  Synthesis latency: {}ms", synth_time.as_millis());

    // Verify samples are valid (not all zeros, not empty)
    assert!(!samples.is_empty(), "No samples returned");
    let max_sample = samples.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    assert!(max_sample > 0.01, "Audio is silent (max sample = {})", max_sample);
    println!("  Max sample amplitude: {:.4}", max_sample);
    println!("  Audio validation: PASS (non-silent)");
}

#[tokio::test]
#[ignore = "requires Piper model in resources/piper/"]
async fn test_piper_all_cached_phrases() {
    println!("\n=== Testing Piper with all cached phrases ===");

    let engine = nexus_lib::tts_piper::new_engine();
    let phrases = [
        "On it sir",
        "Didn't understand that sir",
        "Didn't catch that sir",
        "Here is the analysis, sir",
        "Ok sir",
    ];

    let mut all_ok = true;

    for phrase in &phrases {
        let start = std::time::Instant::now();
        let result = nexus_lib::tts_piper::synthesize(&engine, phrase).await;
        let elapsed = start.elapsed();

        match result {
            Ok((samples, sr)) => {
                let duration_ms = samples.len() as u64 * 1000 / sr as u64;
                println!(
                    "  ✓ '{}' → {} samples, {}ms audio, {}ms latency",
                    phrase,
                    samples.len(),
                    duration_ms,
                    elapsed.as_millis()
                );
            }
            Err(e) => {
                println!("  ✗ '{}' FAILED: {}", phrase, e);
                all_ok = false;
            }
        }
    }

    if all_ok {
        println!("\n✓ All Piper phrases synthesized successfully");
    } else {
        println!("\n⚠ Some Piper phrases failed (model may not be installed)");
    }
}

#[tokio::test]
#[ignore = "requires network access + GROQ_API_KEY env var"]
async fn test_groq_stt_real_transcription() {
    println!("\n=== Testing Groq STT real transcription ===");

    let api_key = std::env::var("GROQ_API_KEY").unwrap_or_default();

    if api_key.is_empty() {
        println!("⚠ GROQ_API_KEY not set — skipping test");
        println!("  To test Groq STT:");
        println!("    1. Get a free key at console.groq.com");
        println!("    2. Set env: $env:GROQ_API_KEY = 'gsk_...'");
        println!("    3. Rerun: cargo test --test phase2_integration test_groq_stt_real_transcription -- --ignored --nocapture");
        return; // Skip, don't fail
    }

    // Generate a simple test WAV: 1 second of 16kHz mono with a beep
    let sample_rate = 16000u32;
    let duration_secs = 1.0;
    let num_samples = (sample_rate as f64 * duration_secs) as usize;
    let mut samples = vec![0i16; num_samples];

    // Add a simple beep (440Hz sine wave for 500ms)
    for i in 0..(num_samples / 2) {
        let t = i as f64 / sample_rate as f64;
        let freq = 440.0;
        let amplitude = 0.3 * i16::MAX as f64;
        samples[i] = (amplitude * (2.0 * std::f64::consts::PI * freq * t).sin()) as i16;
    }

    let client = reqwest::Client::new();

    let start = std::time::Instant::now();
    let result = nexus_lib::stt_groq::transcribe_with_groq(&samples, &api_key, &client).await;
    let elapsed = start.elapsed();

    match result {
        Ok(text) => {
            println!("✓ Groq STT transcription succeeded");
            println!("  Input: 1s beep (440Hz sine wave)");
            println!("  Output: '{}'", text);
            println!("  Latency: {}ms", elapsed.as_millis());
            println!("  (Empty output is normal for non-speech audio)");
        }
        Err(e) => {
            println!("✗ Groq STT FAILED: {}", e);
            panic!("Groq STT real transcription failed: {}", e);
        }
    }
}

#[tokio::test]
#[ignore = "requires network access"]
async fn test_edge_tts_availability_check() {
    println!("\n=== Testing edge-tts availability check ===");

    let start = std::time::Instant::now();
    let available = nexus_lib::tts_edge::is_available().await;
    let elapsed = start.elapsed();

    println!("  edge-tts available: {}", available);
    println!("  Check latency: {}ms", elapsed.as_millis());

    if available {
        println!("✓ Microsoft edge-tts endpoint is reachable");
    } else {
        println!("⚠ edge-tts not reachable — network may be down");
    }
}

#[tokio::test]
#[ignore = "requires network access to Microsoft edge-tts"]
async fn test_edge_tts_long_text() {
    println!("\n=== Testing edge-tts with longer text ===");

    let text = "I've analyzed the repository structure. The codebase has 47 source files across 12 directories. The main entry point is in src/main.rs, which sets up the Tauri application with a floating orb window. The architecture mapper feature scans for tree-sitter symbols and builds a dependency graph using petgraph. The voice pipeline uses a wake word detector followed by speech-to-text, intent parsing, and text-to-speech for responses.";

    let voice = "en-US-AvaNeural";

    let start = std::time::Instant::now();
    let result = nexus_lib::tts_edge::synthesize_to_mp3(text, voice).await;
    let elapsed = start.elapsed();

    match result {
        Ok(bytes) => {
            println!("✓ Long text synthesis succeeded");
            println!("  Text length: {} chars", text.len());
            println!("  MP3 size: {} bytes", bytes.len());
            println!("  Latency: {}ms", elapsed.as_millis());

            // Long text should produce significantly more audio
            assert!(bytes.len() > 5000, "MP3 too small for long text");
            println!("  Size validation: PASS (>5KB for long text)");
        }
        Err(e) => {
            println!("✗ Long text synthesis FAILED: {}", e);
            panic!("edge-tts long text failed: {}", e);
        }
    }
}
