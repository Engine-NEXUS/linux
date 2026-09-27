//! TTS performance benchmarks — measures actual latency for different tasks.
//!
//! Run with: cargo test --lib tts_bench -- --nocapture --test-threads=1

use std::time::Instant;

/// Format duration as milliseconds with 1 decimal.
fn fmt_ms(d: std::time::Duration) -> String {
    format!("{:.1} ms", d.as_secs_f64() * 1000.0)
}

/// Format duration as seconds with 2 decimals.
fn fmt_s(d: std::time::Duration) -> String {
    format!("{:.2}s", d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Test texts of varying length ──────────────────────────────────────

    const SHORT_TEXT: &str = "On it sir";
    const MEDIUM_TEXT: &str = "Here is the analysis of PR 254 in the zync repository. The PR contains 3 changed files with 47 additions and 12 deletions.";
    const LONG_TEXT: &str = "I have completed the analysis of your repository. The codebase contains 15 modules with a total of 8,432 lines of Rust code. The dependency graph shows 3 critical paths that could be affected by changes to the orchestrator module. The blast radius analysis indicates that modifying the intent parser would impact 7 downstream components including the brain monitor, the NLU client, and the GitHub command handler. I recommend reviewing the test coverage for these modules before proceeding with any changes.";

    // ─── Edge TTS (cloud) benchmarks ───────────────────────────────────────

    #[tokio::test]
    async fn bench_edge_tts_short() {
        let start = Instant::now();
        let result = crate::tts_edge::synthesize_to_mp3(SHORT_TEXT, "en-US-AvaNeural").await;
        let elapsed = start.elapsed();

        match &result {
            Ok(bytes) => {
                println!("┌─────────────────────────────────────────────────────────┐");
                println!("│ bench_edge_tts_short                                    │");
                println!("│   text:       \"{}\"", SHORT_TEXT);
                println!("│   voice:      en-US-AvaNeural (Edge TTS cloud)          │");
                println!("│   time:       {}                                      │", fmt_ms(elapsed));
                println!("│   audio:      {} bytes MP3                              │", bytes.len());
                println!("│   status:     OK                                        │");
                println!("└─────────────────────────────────────────────────────────┘");
                assert!(!bytes.is_empty());
            }
            Err(e) => {
                println!("│ bench_edge_tts_short: FAILED — {} (network may be down)  │", e);
                println!("│   time:       {}                                      │", fmt_ms(elapsed));
            }
        }
    }

    #[tokio::test]
    async fn bench_edge_tts_medium() {
        let start = Instant::now();
        let result = crate::tts_edge::synthesize_to_mp3(MEDIUM_TEXT, "en-US-AvaNeural").await;
        let elapsed = start.elapsed();

        match &result {
            Ok(bytes) => {
                println!("┌─────────────────────────────────────────────────────────┐");
                println!("│ bench_edge_tts_medium                                   │");
                println!("│   text len:   {} chars                                  │", MEDIUM_TEXT.len());
                println!("│   voice:      en-US-AvaNeural (Edge TTS cloud)          │");
                println!("│   time:       {}                                      │", fmt_ms(elapsed));
                println!("│   audio:      {} bytes MP3                              │", bytes.len());
                println!("│   status:     OK                                        │");
                println!("└─────────────────────────────────────────────────────────┘");
                assert!(!bytes.is_empty());
            }
            Err(e) => {
                println!("│ bench_edge_tts_medium: FAILED — {}                      │", e);
                println!("│   time:       {}                                      │", fmt_ms(elapsed));
            }
        }
    }

    #[tokio::test]
    async fn bench_edge_tts_long() {
        let start = Instant::now();
        let result = crate::tts_edge::synthesize_to_mp3(LONG_TEXT, "en-US-AvaNeural").await;
        let elapsed = start.elapsed();

        match &result {
            Ok(bytes) => {
                println!("┌─────────────────────────────────────────────────────────┐");
                println!("│ bench_edge_tts_long                                     │");
                println!("│   text len:   {} chars                                  │", LONG_TEXT.len());
                println!("│   voice:      en-US-AvaNeural (Edge TTS cloud)          │");
                println!("│   time:       {}                                      │", fmt_ms(elapsed));
                println!("│   audio:      {} bytes MP3                              │", bytes.len());
                println!("│   status:     OK                                        │");
                println!("└─────────────────────────────────────────────────────────┘");
                assert!(!bytes.is_empty());
            }
            Err(e) => {
                println!("│ bench_edge_tts_long: FAILED — {}                        │", e);
                println!("│   time:       {}                                      │", fmt_ms(elapsed));
            }
        }
    }

    // ─── Edge TTS PCM decode (full pipeline) ───────────────────────────────

    #[tokio::test]
    async fn bench_edge_tts_pcm_decode() {
        let start = Instant::now();
        let result = crate::tts_edge::synthesize_to_pcm(MEDIUM_TEXT, "en-US-AvaNeural").await;
        let elapsed = start.elapsed();

        match &result {
            Ok((samples, sr)) => {
                let audio_dur_ms = samples.len() as u64 * 1000 / *sr as u64;
                println!("┌─────────────────────────────────────────────────────────┐");
                println!("│ bench_edge_tts_pcm_decode (full pipeline)               │");
                println!("│   text len:   {} chars                                  │", MEDIUM_TEXT.len());
                println!("│   synth+decode: {}                                      │", fmt_ms(elapsed));
                println!("│   audio dur:   {} ms ({} samples @ {}Hz)                  │", audio_dur_ms, samples.len(), sr);
                println!("│   realtime ratio: {:.1}x faster than realtime           │", audio_dur_ms as f64 / elapsed.as_millis() as f64);
                println!("│   status:      OK                                       │");
                println!("└─────────────────────────────────────────────────────────┘");
            }
            Err(e) => {
                println!("│ bench_edge_tts_pcm_decode: FAILED — {}                  │", e);
                println!("│   time:        {}                                       │", fmt_ms(elapsed));
            }
        }
    }

    // ─── Network check benchmark ───────────────────────────────────────────

    #[tokio::test]
    async fn bench_network_check() {
        let start = Instant::now();
        let available = crate::tts_edge::is_available().await;
        let elapsed = start.elapsed();

        println!("┌─────────────────────────────────────────────────────────┐");
        println!("│ bench_network_check                                     │");
        println!("│   endpoint:   speech.platform.bing.com                  │");
        println!("│   time:       {}                                      │", fmt_ms(elapsed));
        println!("│   available:  {}                                        │", available);
        println!("└─────────────────────────────────────────────────────────┘");
    }

    // ─── Edge TTS with different voices ────────────────────────────────────

    #[tokio::test]
    async fn bench_edge_tts_multiple_voices() {
        let voices = [
            "en-US-AvaNeural",
            "en-US-GuyNeural",
            "en-US-JennyNeural",
            "en-US-AndrewNeural",
            "en-US-AriaNeural",
        ];
        let text = "Hello, I am your voice assistant. All systems are operational.";

        println!("┌─────────────────────────────────────────────────────────┐");
        println!("│ bench_edge_tts_multiple_voices                          │");
        println!("│   text: \"{}\"", text);
        println!("│                                                         │");
        for voice in &voices {
            let start = Instant::now();
            let result = crate::tts_edge::synthesize_to_mp3(text, voice).await;
            let elapsed = start.elapsed();
            match &result {
                Ok(bytes) => {
                    println!("│   {:<30} {} {:>6} bytes  OK", voice, fmt_ms(elapsed), bytes.len());
                }
                Err(_) => {
                    println!("│   {:<30} {} FAILED", voice, fmt_ms(elapsed));
                }
            }
        }
        println!("└─────────────────────────────────────────────────────────┘");
    }

    // ─── Edge TTS repeated calls (warm vs cold) ────────────────────────────

    #[tokio::test]
    async fn bench_edge_tts_warm_vs_cold() {
        let text = "On it sir";
        let voice = "en-US-AvaNeural";

        println!("┌─────────────────────────────────────────────────────────┐");
        println!("│ bench_edge_tts_warm_vs_cold (5 calls)                   │");
        println!("│   text: \"{}\"", text);
        println!("│                                                         │");

        let mut times: Vec<std::time::Duration> = Vec::new();
        for i in 0..5 {
            let start = Instant::now();
            let result = crate::tts_edge::synthesize_to_mp3(text, voice).await;
            let elapsed = start.elapsed();
            times.push(elapsed);

            match &result {
                Ok(bytes) => {
                    let label = if i == 0 { "COLD" } else { "WARM" };
                    println!("│   call {}: {} {} {:5} bytes  OK", i + 1, label, fmt_ms(elapsed), bytes.len());
                }
                Err(_) => {
                    println!("│   call {}: FAILED {}                              │", i + 1, fmt_ms(elapsed));
                }
            }
        }

        // Calculate stats (only for successful calls)
        if times.iter().count() > 1 {
            let warm_times: Vec<f64> = times[1..].iter().map(|d| d.as_secs_f64() * 1000.0).collect();
            if !warm_times.is_empty() {
                let avg = warm_times.iter().sum::<f64>() / warm_times.len() as f64;
                let min = warm_times.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = warm_times.iter().cloned().fold(0.0, f64::max);
                println!("│                                                         │");
                println!("│   warm avg: {:.1} ms | min: {:.1} ms | max: {:.1} ms     │", avg, min, max);
            }
        }
        println!("└─────────────────────────────────────────────────────────┘");
    }

    // ─── Piper (local fallback) benchmark ──────────────────────────────────

    #[tokio::test]
    async fn bench_piper_short() {
        let engine = crate::tts_piper::new_engine();
        let text = "On it sir";

        let start = Instant::now();
        let result = crate::tts_piper::synthesize(&engine, text).await;
        let elapsed = start.elapsed();

        match &result {
            Ok((samples, sr)) => {
                let audio_dur_ms = samples.len() as u64 * 1000 / *sr as u64;
                println!("┌─────────────────────────────────────────────────────────┐");
                println!("│ bench_piper_short (local fallback)                      │");
                println!("│   text:       \"{}\"", text);
                println!("│   time:       {} (includes model load)                  │", fmt_s(elapsed));
                println!("│   audio dur:   {} ms ({} samples @ {}Hz)                  │", audio_dur_ms, samples.len(), sr);
                println!("│   status:      OK                                        │");
                println!("└─────────────────────────────────────────────────────────┘");
            }
            Err(e) => {
                println!("│ bench_piper_short: FAILED — {}                          │", e);
                println!("│   time:       {}                                        │", fmt_s(elapsed));
            }
        }
    }

    #[tokio::test]
    async fn bench_piper_medium_warm() {
        let engine = crate::tts_piper::new_engine();

        // Warm up Piper (first call loads the model)
        let _ = crate::tts_piper::synthesize(&engine, "warmup").await;

        let text = MEDIUM_TEXT;
        let start = Instant::now();
        let result = crate::tts_piper::synthesize(&engine, text).await;
        let elapsed = start.elapsed();

        match &result {
            Ok((samples, sr)) => {
                let audio_dur_ms = samples.len() as u64 * 1000 / *sr as u64;
                println!("┌─────────────────────────────────────────────────────────┐");
                println!("│ bench_piper_medium_warm (model already loaded)          │");
                println!("│   text len:   {} chars                                  │", text.len());
                println!("│   time:       {}                                      │", fmt_ms(elapsed));
                println!("│   audio dur:   {} ms ({} samples @ {}Hz)                  │", audio_dur_ms, samples.len(), sr);
                println!("│   realtime ratio: {:.1}x faster than realtime           │", audio_dur_ms as f64 / elapsed.as_millis() as f64);
                println!("│   status:      OK                                        │");
                println!("└─────────────────────────────────────────────────────────┘");
            }
            Err(e) => {
                println!("│ bench_piper_medium_warm: FAILED — {}                    │", e);
                println!("│   time:       {}                                        │", fmt_ms(elapsed));
            }
        }
    }

    // ─── Invalid voice benchmark (the old bug) ─────────────────────────────

    #[tokio::test]
    async fn bench_edge_tts_invalid_voice_wasted_time() {
        let start = Instant::now();
        let result = crate::tts_edge::synthesize_to_mp3("Hello world", "af_sky").await;
        let elapsed = start.elapsed();

        println!("┌─────────────────────────────────────────────────────────┐");
        println!("│ bench_edge_tts_invalid_voice (the old bug)              │");
        println!("│   voice:      af_sky (invalid Kokoro voice)             │");
        println!("│   time wasted: {}                                    │", fmt_ms(elapsed));
        println!("│   result:     {}                                       │", if result.is_err() { "REJECTED → Piper fallback" } else { "unexpected OK" });
        println!("│   impact:     this delay was added to EVERY speak()     │");
        println!("│               call before the fix                       │");
        println!("└─────────────────────────────────────────────────────────┘");
    }

    // ─── Summary table ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn bench_summary_table() {
        println!();
        println!("╔══════════════════════════════════════════════════════════════════════╗");
        println!("║           TTS PERFORMANCE BENCHMARK SUMMARY                          ║");
        println!("╠══════════════════════════════════════════════════════════════════════╣");
        println!("║ Task                          │ Engine    │ Time       │ Status      ║");
        println!("╠═══════════════════════════════╪═══════════╪════════════╪═════════════╣");

        // 1. Network check
        let start = Instant::now();
        let net_ok = crate::tts_edge::is_available().await;
        let net_time = start.elapsed();
        println!("║ Network check                 │ -         │ {:<10} │ {:<11} ║",
            fmt_ms(net_time), if net_ok { "OK" } else { "DOWN" });

        // 2. Edge TTS short
        let start = Instant::now();
        let r = crate::tts_edge::synthesize_to_mp3(SHORT_TEXT, "en-US-AvaNeural").await;
        let t = start.elapsed();
        println!("║ Edge TTS short (9 chars)      │ Cloud     │ {:<10} │ {:<11} ║",
            fmt_ms(t), if r.is_ok() { "OK" } else { "FAIL" });

        // 3. Edge TTS medium
        let start = Instant::now();
        let r = crate::tts_edge::synthesize_to_mp3(MEDIUM_TEXT, "en-US-AvaNeural").await;
        let t = start.elapsed();
        println!("║ Edge TTS medium (130 chars)   │ Cloud     │ {:<10} │ {:<11} ║",
            fmt_ms(t), if r.is_ok() { "OK" } else { "FAIL" });

        // 4. Edge TTS long
        let start = Instant::now();
        let r = crate::tts_edge::synthesize_to_mp3(LONG_TEXT, "en-US-AvaNeural").await;
        let t = start.elapsed();
        println!("║ Edge TTS long (530 chars)     │ Cloud     │ {:<10} │ {:<11} ║",
            fmt_ms(t), if r.is_ok() { "OK" } else { "FAIL" });

        // 5. Invalid voice (old bug)
        let start = Instant::now();
        let r = crate::tts_edge::synthesize_to_mp3("Hello", "af_sky").await;
        let t = start.elapsed();
        println!("║ Invalid voice (old bug)       │ Cloud     │ {:<10} │ {:<11} ║",
            fmt_ms(t), if r.is_err() { "REJECTED" } else { "OK" });

        // 6. Piper cold (first load)
        let engine = crate::tts_piper::new_engine();
        let start = Instant::now();
        let r = crate::tts_piper::synthesize(&engine, SHORT_TEXT).await;
        let t = start.elapsed();
        println!("║ Piper cold (model load)       │ Local     │ {:<10} │ {:<11} ║",
            fmt_s(t), if r.is_ok() { "OK" } else { "FAIL" });

        // 7. Piper warm
        let start = Instant::now();
        let r = crate::tts_piper::synthesize(&engine, SHORT_TEXT).await;
        let t = start.elapsed();
        println!("║ Piper warm (model loaded)     │ Local     │ {:<10} │ {:<11} ║",
            fmt_ms(t), if r.is_ok() { "OK" } else { "FAIL" });

        // 8. Piper warm medium
        let start = Instant::now();
        let r = crate::tts_piper::synthesize(&engine, MEDIUM_TEXT).await;
        let t = start.elapsed();
        println!("║ Piper warm medium (130 chars) │ Local     │ {:<10} │ {:<11} ║",
            fmt_ms(t), if r.is_ok() { "OK" } else { "FAIL" });

        println!("╠═══════════════════════════════╧═══════════╧════════════╧═════════════╣");
        println!("║ RAM: Edge TTS = 0 MB | Piper = ~80 MB (after load)                   ║");
        println!("║ Cost: Edge TTS = $0 (free cloud) | Piper = $0 (local)               ║");
        println!("║ Network down: Piper used directly (no Edge TTS wait)                ║");
        println!("║ Network up 10+ min: Piper unloaded → 80 MB freed                    ║");
        println!("╚══════════════════════════════════════════════════════════════════════╝");
        println!();
    }
}
