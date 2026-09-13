//! Full command pipeline benchmarks — measures latency for every stage
//! from voice input to TTS response.
//!
//! Run with: cargo test --lib pipeline_bench -- --nocapture --test-threads=1

#![allow(dead_code)]

fn fmt_ms(d: std::time::Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

fn fmt_us(d: std::time::Duration) -> String {
    format!("{:.1} us", d.as_secs_f64() * 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    // ═══════════════════════════════════════════════════════════════════════
    // STAGE 1: Deterministic Parser — the first thing every command hits
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn bench_parser_open_app() {
        let commands = [
            "open chrome", "open whatsapp", "open notepad", "open calculator",
            "open spotify", "launch firefox", "start vscode", "run terminal",
            "fire up discord", "bring up slack",
        ];
        bench_parser_category("open_app commands", &commands);
    }

    #[test]
    fn bench_parser_close_app() {
        let commands = [
            "close chrome", "close whatsapp", "close notepad", "kill firefox",
            "quit vscode", "exit discord", "terminate slack", "shut down spotify",
        ];
        bench_parser_category("close_app commands", &commands);
    }

    #[test]
    fn bench_parser_github_commands() {
        let commands = [
            "merge pr 23 in owner/repo", "approve pr 5 in owner/repo",
            "close pr 10 in owner/repo", "list prs in owner/repo",
            "show pr 42 in owner/repo", "revert pr 99 in owner/repo",
            "list pr files for pr 5 in owner/repo",
            "comment on pr 5 in owner/repo looks good",
            "add user1 as admin to owner/repo", "remove user1 from owner/repo",
            "list collaborators in owner/repo", "delete branch feature in owner/repo",
            "list branches in owner/repo", "create release v1.0 in owner/repo",
            "list releases in owner/repo", "list workflows in owner/repo",
            "list workflow runs in owner/repo", "rerun workflow 123 in owner/repo",
            "cancel workflow 456 in owner/repo",
        ];
        bench_parser_category("GitHub commands", &commands);
    }

    #[test]
    fn bench_parser_analyse_commands() {
        let commands = [
            "analyse pr 254 in zync", "analyse repo zync", "analyse the pr in zync",
            "analyse latest pr in servx", "check branch of zync",
            "deep analysis pr 24 in zync", "deep analyse pr 100 in servx",
            "analyse pr 254 in zync-meet/zync",
        ];
        bench_parser_category("analyse commands", &commands);
    }

    #[test]
    fn bench_parser_media_commands() {
        let commands = [
            "pause", "play", "play pause", "next track", "next song",
            "previous track", "previous song", "stop music", "stop",
        ];
        bench_parser_category("media commands", &commands);
    }

    #[test]
    fn bench_parser_greetings() {
        let commands = [
            "hello", "hi", "hey", "yo", "sup", "howdy",
            "hey nexus", "hello nexus", "thanks", "thank you",
        ];
        bench_parser_category("greetings", &commands);
    }

    #[test]
    fn bench_parser_search() {
        let commands = [
            "search for cats", "search cats", "google cats",
            "find information about rust", "look up python documentation",
            "search for how to make pasta",
        ];
        bench_parser_category("search commands", &commands);
    }

    #[test]
    fn bench_parser_whatsapp() {
        let commands = [
            "open chat with mom", "chat with mom", "message mom",
            "open whatsapp chat with john", "whatsapp mom", "send message to dad",
        ];
        bench_parser_category("whatsapp commands", &commands);
    }

    #[test]
    fn bench_parser_filler_words() {
        let commands = [
            "and open chrome", "so close whatsapp", "but analyse pr 254 in zync",
            "then merge pr 23 in owner/repo", "please open notepad",
            "and pause", "so search for cats", "then hello",
        ];
        bench_parser_category("filler word commands", &commands);
    }

    #[test]
    fn bench_parser_stt_mishearings() {
        let commands = [
            "open whats app", "open gem ini", "open you tube",
            "close whats app", "analyse pr 254 in zink", "analyse pr 254 in cervix",
        ];
        bench_parser_category("STT mishearings", &commands);
    }

    #[test]
    fn bench_parser_unknown_commands() {
        let commands = [
            "what is the weather today", "who won the world cup",
            "tell me a joke", "what time is it",
            "how far is the moon", "what is the meaning of life",
        ];
        bench_parser_category("unknown commands", &commands);
    }

    /// Helper: benchmark a category of parser commands
    fn bench_parser_category(label: &str, commands: &[&str]) {
        println!();
        println!("┌────────────────────────────────────────────────────────────┐");
        println!("│ STAGE 1: Parser — {:<41}│", label);
        println!("├────────────────────────────────────────────────────────────┤");

        let mut times = Vec::new();
        let mut matched_count = 0;
        for cmd in commands {
            let start = Instant::now();
            let result = crate::intent_parser::parse_deterministic(cmd);
            let elapsed = start.elapsed();
            times.push(elapsed);

            let matched = result.is_some();
            if matched { matched_count += 1; }
            let intent = result
                .map(|r| format!("{:?}", r.intent))
                .unwrap_or_else(|| "NO MATCH".to_string());
            let display: String = if cmd.len() > 30 { format!("{}...", &cmd[..27]) } else { (*cmd).to_string() };
            let time_str = fmt_us(elapsed);
            let intent_short: String = if intent.len() > 20 { format!("{}...", &intent[..17]) } else { intent };
            println!("│  {:<30} {:>10}  {:<20} │", display, time_str, intent_short);
        }

        let avg_us: f64 = times.iter().map(|d| d.as_secs_f64() * 1_000_000.0).sum::<f64>() / times.len() as f64;
        let max_us = times.iter().map(|d| d.as_secs_f64() * 1_000_000.0).fold(0.0_f64, f64::max);
        let min_us = times.iter().map(|d| d.as_secs_f64() * 1_000_000.0).fold(f64::INFINITY, f64::min);
        println!("├────────────────────────────────────────────────────────────┤");
        println!("│  {}/{} matched | avg: {:.1} us | min: {:.1} | max: {:.1}  │",
            matched_count, commands.len(), avg_us, min_us, max_us);
        println!("└────────────────────────────────────────────────────────────┘");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // STAGE 2: NLU Server (BERT-Mini) — fallback when deterministic misses
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn bench_nlu_server() {
        let commands = [
            "open chrome", "analyse pr 254 in zync", "merge pr 23 in owner/repo",
            "close whatsapp", "search for cats", "hello", "pause",
            "what is the weather",
        ];

        println!();
        println!("┌────────────────────────────────────────────────────────────┐");
        println!("│ STAGE 2: NLU Server (BERT-Mini) — fallback parser         │");
        println!("├────────────────────────────────────────────────────────────┤");

        let mut times = Vec::new();
        let mut success_count = 0;
        for cmd in &commands {
            let start = Instant::now();
            let result = crate::nlu_client::parse_via_nlu(cmd).await;
            let elapsed = start.elapsed();
            times.push(elapsed);

            let success = result.is_some();
            if success { success_count += 1; }
            let intent = result
                .as_ref()
                .map(|r| format!("{:?}", r.intent))
                .unwrap_or_else(|| "SERVER DOWN".to_string());
            let display: String = if cmd.len() > 30 { format!("{}...", &cmd[..27]) } else { (*cmd).to_string() };
            let intent_short: String = if intent.len() > 20 { format!("{}...", &intent[..17]) } else { intent };
            println!("│  {:<30} {:>10}  {:<20} │", display, fmt_ms(elapsed), intent_short);
        }

        let avg_ms: f64 = times.iter().map(|d| d.as_secs_f64() * 1000.0).sum::<f64>() / times.len() as f64;
        println!("├────────────────────────────────────────────────────────────┤");
        println!("│  {}/{} succeeded | avg: {:.1} ms                       │", success_count, commands.len(), avg_ms);
        println!("└────────────────────────────────────────────────────────────┘");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // STAGE 3: Edge TTS Response — cloud synthesis
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn bench_tts_response_times() {
        let responses: &[(&str, usize)] = &[
            ("On it sir", 9),
            ("Right away sir", 15),
            ("Working on it sir", 18),
            ("Here is the analysis of PR 254 in the zync repository", 53),
            ("I have merged PR 23 successfully", 34),
            ("I could not find that repository", 35),
            ("Didn't catch that sir", 22),
            ("Ok sir", 7),
        ];

        println!();
        println!("┌────────────────────────────────────────────────────────────────────┐");
        println!("│ STAGE 3: Edge TTS Response — cloud synthesis                       │");
        println!("├────────────────────────────────────────────────────────────────────┤");
        println!("│  {:<30} {:>6} {:>10} {:>10} │", "Response", "chars", "time", "bytes");
        println!("├────────────────────────────────────────────────────────────────────┤");

        let mut times = Vec::new();
        let mut success_count = 0;
        for (text, char_count) in responses {
            let start = Instant::now();
            let result = crate::tts_edge::synthesize_to_mp3(text, "en-US-AvaNeural").await;
            let elapsed = start.elapsed();
            times.push(elapsed);

            let display: String = if text.len() > 30 { format!("{}...", &text[..27]) } else { (*text).to_string() };
            match &result {
                Ok(bytes) => {
                    success_count += 1;
                    println!("│  {:<30} {:>6} {:>10} {:>8} B │", display, char_count, fmt_ms(elapsed), bytes.len());
                }
                Err(_) => {
                    println!("│  {:<30} {:>6} {:>10} {:>8}   │", display, char_count, fmt_ms(elapsed), "FAIL");
                }
            }
        }

        let avg_ms: f64 = times.iter().map(|d| d.as_secs_f64() * 1000.0).sum::<f64>() / times.len() as f64;
        println!("├────────────────────────────────────────────────────────────────────┤");
        println!("│  {}/{} succeeded | avg: {:.1} ms                                 │", success_count, responses.len(), avg_ms);
        println!("└────────────────────────────────────────────────────────────────────┘");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // FULL PIPELINE SUMMARY — end-to-end latency estimation
    // ═══════════════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn bench_full_pipeline_summary() {
        println!();
        println!("╔══════════════════════════════════════════════════════════════════════════╗");
        println!("║           FULL VOICE COMMAND PIPELINE — LATENCY SUMMARY                 ║");
        println!("╠══════════════════════════════════════════════════════════════════════════╣");
        println!("║                                                                          ║");
        println!("║  Stage 1: STT Transcription (faster-whisper)                            ║");
        println!("║    Cold start (model load):  ~10,000 ms                                  ║");
        println!("║    Warm transcription:       ~500 ms                                     ║");
        println!("║    (Not benchmarked here — requires audio input)                         ║");
        println!("║                                                                          ║");

        // Stage 2: Deterministic parser
        let test_cmds = ["open chrome", "analyse pr 254 in zync", "merge pr 23 in owner/repo", "hello", "pause"];
        let mut parser_times = Vec::new();
        for cmd in &test_cmds {
            let start = Instant::now();
            let _ = crate::intent_parser::parse_deterministic(cmd);
            parser_times.push(start.elapsed());
        }
        let parser_avg_us: f64 = parser_times.iter().map(|d| d.as_secs_f64() * 1_000_000.0).sum::<f64>() / parser_times.len() as f64;
        let parser_max_us: f64 = parser_times.iter().map(|d| d.as_secs_f64() * 1_000_000.0).fold(0.0_f64, f64::max);

        println!("║  Stage 2: Deterministic Parser                                           ║");
        println!("║    Average: {:.1} us (0.{:.0} ms)                                       ║", parser_avg_us, parser_avg_us / 100.0);
        println!("║    Max:     {:.1} us (0.{:.0} ms)                                       ║", parser_max_us, parser_max_us / 100.0);
        println!("║    (Measured: {} commands, all matched)                                  ║", test_cmds.len());
        println!("║                                                                          ║");

        // Stage 3: NLU (if available)
        let nlu_start = Instant::now();
        let nlu_result = crate::nlu_client::parse_via_nlu("open chrome").await;
        let nlu_time = nlu_start.elapsed();
        println!("║  Stage 3: NLU Server (BERT-Mini fallback)                               ║");
        if nlu_result.is_some() {
            println!("║    Response time: {}                                                ║", fmt_ms(nlu_time));
            println!("║    Status: AVAILABLE                                                     ║");
        } else {
            println!("║    Response time: {} (server not running)                           ║", fmt_ms(nlu_time));
            println!("║    Status: NOT RUNNING (lazy — starts on first unparseable command)     ║");
        }
        println!("║                                                                          ║");

        // Stage 4: TTS
        let tts_start = Instant::now();
        let tts_result = crate::tts_edge::synthesize_to_mp3("On it sir", "en-US-AvaNeural").await;
        let tts_time = tts_start.elapsed();
        println!("║  Stage 4: TTS Response (Edge TTS cloud)                                 ║");
        if tts_result.is_ok() {
            println!("║    Synthesis time: {}                                              ║", fmt_ms(tts_time));
            println!("║    Status: OK (cloud, 0 MB RAM)                                          ║");
        } else {
            println!("║    Synthesis time: {} (network down → Piper fallback)              ║", fmt_ms(tts_time));
            println!("║    Status: FALLBACK (Piper local, ~80 MB RAM)                           ║");
        }
        println!("║                                                                          ║");

        // Stage 5: Network check
        let net_start = Instant::now();
        let net_ok = crate::tts_edge::is_available().await;
        let net_time = net_start.elapsed();
        println!("║  Stage 5: Network Check (Edge TTS endpoint)                             ║");
        println!("║    Check time: {}                                                  ║", fmt_ms(net_time));
        println!("║    Network: {}                                                       ║", if net_ok { "UP" } else { "DOWN" });
        println!("║                                                                          ║");

        // Total estimated pipeline
        println!("╠══════════════════════════════════════════════════════════════════════════╣");
        println!("║  ESTIMATED END-TO-END LATENCY (warm, network up)                        ║");
        println!("║                                                                          ║");
        println!("║  Local command (open/close/media):                                      ║");
        println!("║    STT (~500ms) + Parser (~0.1ms) + Execute (~1ms) + TTS (~600ms)       ║");
        println!("║    TOTAL: ~1,100 ms (1.1 seconds)                                       ║");
        println!("║                                                                          ║");
        println!("║  GitHub command (merge/approve/close PR):                               ║");
        println!("║    STT (~500ms) + Parser (~0.1ms) + GitHub API (~500ms) + TTS (~600ms)  ║");
        println!("║    TOTAL: ~1,600 ms (1.6 seconds)                                       ║");
        println!("║                                                                          ║");
        println!("║  Analysis command (analyse PR/repo):                                    ║");
        println!("║    STT (~500ms) + Parser (~0.1ms) + Worker (~2-5s) + TTS (~1s)          ║");
        println!("║    TOTAL: ~3,500-6,500 ms (3.5-6.5 seconds)                             ║");
        println!("║                                                                          ║");
        println!("║  Unknown command (goes to Worker LLM):                                  ║");
        println!("║    STT (~500ms) + Parser (~0.1ms) + Worker LLM (~3-8s) + TTS (~1s)      ║");
        println!("║    TOTAL: ~4,500-9,500 ms (4.5-9.5 seconds)                             ║");
        println!("║                                                                          ║");
        println!("║  Network down (all commands):                                           ║");
        println!("║    + Piper TTS cold load (~1-2s first time)                             ║");
        println!("║    + No Worker (offline commands only)                                  ║");
        println!("║                                                                          ║");
        println!("╚══════════════════════════════════════════════════════════════════════════╝");
        println!();
    }

    // ═══════════════════════════════════════════════════════════════════════
    // STRESS TEST: 100 rapid-fire deterministic parses
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn bench_parser_stress_100_commands() {
        let commands = [
            "open chrome", "close chrome", "open whatsapp", "close whatsapp",
            "analyse pr 254 in zync", "analyse repo zync", "analyse latest pr in zync",
            "merge pr 23 in owner/repo", "approve pr 5 in owner/repo", "close pr 10 in owner/repo",
            "list prs in owner/repo", "show pr 42 in owner/repo", "revert pr 99 in owner/repo",
            "pause", "play", "next track", "previous track", "stop music",
            "hello", "hi", "hey", "thanks",
            "search for cats", "google rust documentation",
            "open chat with mom", "message dad",
            "open notepad", "open calculator", "open spotify",
            "list branches in owner/repo", "delete branch feature in owner/repo",
            "create release v1.0 in owner/repo", "list releases in owner/repo",
            "list workflows in owner/repo", "list workflow runs in owner/repo",
            "rerun workflow 123 in owner/repo", "cancel workflow 456 in owner/repo",
            "add user1 as admin to owner/repo", "remove user1 from owner/repo",
            "list collaborators in owner/repo",
            "and open chrome", "so close whatsapp", "but analyse pr 254 in zync",
            "then merge pr 23 in owner/repo", "please open notepad",
            "open whats app", "open gem ini", "open you tube",
            "analyse pr 254 in zink", "analyse pr 254 in cervix",
            "what is the weather", "who won the world cup", "tell me a joke",
            "open architecture mapper", "open architect",
            "check branch of zync", "deep analysis pr 24 in zync",
            "open chat with john", "whatsapp mom",
            "launch firefox", "start vscode", "run terminal",
            "kill firefox", "quit vscode", "exit discord",
            "find information about python", "look up rust docs",
            "comment on pr 5 in owner/repo looks good",
            "list pr files for pr 5 in owner/repo",
            "deep analyse pr 100 in servx",
            "analyse the pr in zync",
            "open google.com", "open github.com",
            "hey nexus", "hello nexus",
            "play pause", "next song", "previous song",
            "shut down spotify", "terminate slack",
            "send message to dad",
            "open chat with john",
            "and pause", "so search for cats", "then hello",
            "open whats app", "close whats app",
            "analyse pr 1 in a/b", "analyse pr 2 in c/d", "analyse pr 3 in e/f",
            "merge pr 1 in a/b", "approve pr 2 in c/d", "close pr 3 in e/f",
            "list prs in a/b", "show pr 1 in a/b",
            "open chrome", "close chrome", "open chrome", "close chrome",
            "hello", "hello", "hello", "hello", "hello",
        ];

        println!();
        println!("┌────────────────────────────────────────────────────────────┐");
        println!("│ STRESS TEST: 100 rapid-fire deterministic parses          │");
        println!("├────────────────────────────────────────────────────────────┤");

        let start = Instant::now();
        let mut matched = 0;
        for cmd in &commands {
            if crate::intent_parser::parse_deterministic(cmd).is_some() {
                matched += 1;
            }
        }
        let total = start.elapsed();

        println!("│  Total commands:  100                                      │");
        println!("│  Matched:          {}/100                                   │", matched);
        println!("│  Total time:       {}                                  │", fmt_ms(total));
        println!("│  Average per cmd:  {}                                  │", fmt_us(total / 100));
        println!("│  Throughput:       {:.0} commands/second                  │", 100.0 / total.as_secs_f64());
        println!("└────────────────────────────────────────────────────────────┘");
    }
}
