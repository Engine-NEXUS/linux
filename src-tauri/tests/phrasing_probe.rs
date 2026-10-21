//! Phrasing-coverage probe (research harness, `#[ignore]` — never runs in CI).
//!
//! Reads JSONL rows `{"id": .., "text": ..}` from `$NEXUS_PROBE_IN`, runs each through
//! `parse_deterministic` (the first, zero-model stage of the voice pipeline) and writes JSONL rows
//! `{"id","text","label","intent","confidence","us"}` to `$NEXUS_PROBE_OUT`.
//! `label` is the resolved label ("none" = clean miss), `intent` the Debug form (for slot checks),
//! `us` the parse time in microseconds.
//!
//! Run: NEXUS_PROBE_IN=in.jsonl NEXUS_PROBE_OUT=out.jsonl \
//!      cargo test --test phrasing_probe -- --ignored --nocapture
//! Used by docs/features/103 (command-understanding plan) to measure coverage, not to gate builds.

use nexus_lib::intent_parser::{intent_to_label, parse_deterministic, ParsedIntent};
use std::io::Write;

fn resolved_label(r: &Option<nexus_lib::intent_parser::ParseResult>) -> String {
    match r {
        None => "none".to_string(),
        Some(r) => match &r.intent {
            ParsedIntent::NluResult { intent, .. } => intent.clone(),
            other => intent_to_label(other).to_string(),
        },
    }
}

#[test]
#[ignore]
fn phrasing_probe() {
    let inp = std::env::var("NEXUS_PROBE_IN").expect("set NEXUS_PROBE_IN");
    let outp = std::env::var("NEXUS_PROBE_OUT").expect("set NEXUS_PROBE_OUT");
    let data = std::fs::read_to_string(&inp).expect("read input");
    let mut out = std::fs::File::create(&outp).expect("create output");
    let mut n = 0usize;
    for line in data.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).expect("json row");
        let text = v["text"].as_str().unwrap_or("");
        let t0 = std::time::Instant::now();
        let r = parse_deterministic(text);
        let us = t0.elapsed().as_micros() as u64;
        let row = serde_json::json!({
            "id": v["id"],
            "text": text,
            "label": resolved_label(&r),
            "intent": r.as_ref().map(|r| format!("{:?}", r.intent)).unwrap_or_default(),
            "confidence": r.as_ref().map(|r| r.confidence).unwrap_or(0.0),
            "us": us,
        });
        writeln!(out, "{row}").unwrap();
        n += 1;
    }
    println!("phrasing_probe: {n} rows -> {outp}");
}
