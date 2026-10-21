//! Wake replay — scores the REAL wake engine (`wake_eval`, same code as the app)
//! over recordings and prints recall, false fires per hour, and the threshold
//! trade-off. This is the source of truth for wake accuracy numbers; the
//! Python testers re-implement the pipeline and have drifted.
//!
//! Usage (run from src-tauri; use --release, debug is ~20x slower):
//!   cargo run --release --example wake_replay -- --manifest ../wake_word_data/manifest.jsonl --split test
//!   cargo run --release --example wake_replay -- --pos ../wake_camp/raw/nexus --neg ../wake_camp/raw/negatives
//!
//! Options:
//!   --manifest FILE    wake_word_data manifest (rows: audio, label, split, condition/environment)
//!   --split NAME       manifest split to score (default: test)
//!   --pos DIR          folder of positive WAVs, recursive (repeatable; group = parent folder)
//!   --neg DIR          folder of negative WAVs, recursive (repeatable)
//!   --model FILE       candidate classifier .onnx instead of the shipped nexus.onnx
//!   --thresholds A:B:S threshold sweep (default 0.10:0.95:0.05; the profile threshold is always added)
//!   --target-fah F     stage-1 false fires per hour budget for the recommended threshold (default 0.5)
//!   --offenders N      false fires to list for hard-negative mining (default 15)
//!   --limit N          score at most N files per label (smoke runs)
//!   --json FILE        write the full report as JSON
//!
//! Stage 1 only (speaker check and transcript verify are not modelled). Read-only:
//! audio never leaves this machine and nothing is written except --json.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use nexus_lib::wake_eval::{self, Label, Point, ReplayEngine, Scored};
use serde::Serialize;

struct Args {
    manifest: Option<PathBuf>,
    split: String,
    pos: Vec<PathBuf>,
    neg: Vec<PathBuf>,
    model: Option<PathBuf>,
    thresholds: (f32, f32, f32),
    target_fah: f64,
    offenders: usize,
    limit: Option<usize>,
    json: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        manifest: None,
        split: "test".into(),
        pos: vec![],
        neg: vec![],
        model: None,
        thresholds: (0.10, 0.95, 0.05),
        target_fah: 0.5,
        offenders: 15,
        limit: None,
        json: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--manifest" => a.manifest = Some(val()?.into()),
            "--split" => a.split = val()?,
            "--pos" => a.pos.push(val()?.into()),
            "--neg" => a.neg.push(val()?.into()),
            "--model" => a.model = Some(val()?.into()),
            "--thresholds" => {
                let v = val()?;
                let p: Vec<f32> = v.split(':').filter_map(|x| x.parse().ok()).collect();
                if p.len() != 3 || p[2] <= 0.0 || p[0] > p[1] {
                    return Err(format!("--thresholds expects LO:HI:STEP, got {v}"));
                }
                a.thresholds = (p[0], p[1], p[2]);
            }
            "--target-fah" => a.target_fah = val()?.parse().map_err(|_| "bad --target-fah")?,
            "--offenders" => a.offenders = val()?.parse().map_err(|_| "bad --offenders")?,
            "--limit" => a.limit = Some(val()?.parse().map_err(|_| "bad --limit")?),
            "--json" => a.json = Some(val()?.into()),
            "-h" | "--help" => {
                println!("{}", include_str!("wake_replay.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            other => return Err(format!("unknown option {other} (see --help)")),
        }
    }
    if a.manifest.is_none() && a.pos.is_empty() && a.neg.is_empty() {
        return Err("give --manifest or --pos/--neg folders (see --help)".into());
    }
    Ok(a)
}

/// (path, label, group)
type Job = (PathBuf, Label, String);

fn wavs_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            wavs_under(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
            out.push(p);
        }
    }
}

fn folder_jobs(dirs: &[PathBuf], label: Label, jobs: &mut Vec<Job>) {
    for d in dirs {
        let mut files = vec![];
        wavs_under(d, &mut files);
        for f in files {
            let group = f
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            jobs.push((f, label, group));
        }
    }
}

fn manifest_jobs(path: &Path, split: &str, jobs: &mut Vec<Job>) -> Result<(), String> {
    let root = path.parent().unwrap_or(Path::new("."));
    let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("{}:{}: {e}", path.display(), n + 1))?;
        if row["split"].as_str() != Some(split) {
            continue;
        }
        let audio = row["audio"].as_str().ok_or_else(|| format!("line {}: no audio", n + 1))?;
        let label = match row["label"].as_str() {
            Some("positive") => Label::Positive,
            Some("negative") | Some("background") => Label::Negative,
            other => return Err(format!("line {}: bad label {other:?}", n + 1)),
        };
        let group = row["condition"]
            .as_str()
            .or_else(|| row["environment"].as_str())
            .unwrap_or("")
            .to_string();
        jobs.push((root.join(audio.replace('\\', "/")), label, group));
    }
    Ok(())
}

#[derive(Serialize)]
struct Report {
    model: String,
    profile_threshold: f32,
    target_fa_per_hour: f64,
    positives: usize,
    short_negative_clips: usize,
    negative_stream_hours: f64,
    skipped: Vec<String>,
    at_profile_threshold: Point,
    recommended: Option<Point>,
    sweep: Vec<Point>,
    group_recall: Vec<wake_eval::GroupRecall>,
    negative_groups: BTreeMap<String, (usize, usize)>,
    misses: Vec<wake_eval::Miss>,
    offenders: Vec<wake_eval::Offender>,
    audio_hours_scored: f64,
    seconds: f64,
}

fn fah(p: &Point) -> String {
    p.fa_per_hour.map_or("   n/a".into(), |f| format!("{f:6.2}"))
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("wake_replay: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args) {
        eprintln!("wake_replay failed: {e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let mut jobs: Vec<Job> = vec![];
    if let Some(m) = &args.manifest {
        manifest_jobs(m, &args.split, &mut jobs)?;
    }
    folder_jobs(&args.pos, Label::Positive, &mut jobs);
    folder_jobs(&args.neg, Label::Negative, &mut jobs);
    if let Some(limit) = args.limit {
        let (mut np, mut nn) = (0, 0);
        jobs.retain(|(_, l, _)| {
            let c = if *l == Label::Positive { &mut np } else { &mut nn };
            *c += 1;
            *c <= limit
        });
    }
    if jobs.is_empty() {
        return Err("no recordings matched".into());
    }

    let mut engine = ReplayEngine::load(args.model.as_deref()).map_err(|e| e.to_string())?;
    let model = args
        .model
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "shipped nexus.onnx".into());
    let prof = engine.threshold();
    println!(
        "Model: {model}\nProfile: threshold {prof:.2}, high-pass {:.1} Hz, gate {:.4}, pre-gain {:.2}x",
        engine.profile().highpass_cutoff_hz,
        engine.profile().silence_rms_threshold,
        engine.profile().pre_gain
    );

    let started = Instant::now();
    let mut set: Vec<Scored> = Vec::with_capacity(jobs.len());
    let mut skipped = vec![];
    let mut audio_s = 0.0;
    let total = jobs.len();
    for (i, (path, label, group)) in jobs.into_iter().enumerate() {
        let audio = match wake_eval::read_wav(&path) {
            Ok(a) => a,
            Err(e) => {
                skipped.push(e.to_string());
                continue;
            }
        };
        let (samples, duration_s) = wake_eval::prepare(&audio);
        audio_s += duration_s;
        let frames = engine.frames(&samples);
        set.push(Scored { path: path.display().to_string(), label, group, duration_s, frames });
        if (i + 1) % 200 == 0 || i + 1 == total {
            eprint!("\r  scored {}/{} recordings ({:.2} h audio)   ", i + 1, total, audio_s / 3600.0);
        }
    }
    eprintln!();
    let seconds = started.elapsed().as_secs_f64();

    let (lo, hi, step) = args.thresholds;
    let mut thresholds = vec![];
    let mut t = lo;
    while t <= hi + 1e-6 {
        thresholds.push((t * 1000.0).round() / 1000.0);
        t += step;
    }
    if !thresholds.iter().any(|&x| (x - prof).abs() < 1e-4) {
        thresholds.push(prof);
        thresholds.sort_by(|a, b| a.partial_cmp(b).unwrap());
    }
    let sweep = wake_eval::sweep(&set, &thresholds);
    let at_prof = wake_eval::evaluate_at(&set, prof);
    let recommended = wake_eval::best_for_target(&sweep, args.target_fah);

    println!(
        "\nScored {} positives, {} short negative clips, {:.2} h of negative streams in {:.0}s ({:.0}x real time)",
        at_prof.pos_total,
        at_prof.neg_clips,
        at_prof.neg_hours,
        seconds,
        audio_s / seconds.max(1e-9)
    );
    if !skipped.is_empty() {
        println!("Skipped {} unreadable files (first: {})", skipped.len(), skipped[0]);
    }

    println!("\n  thr | recall            | short neg clips fired | stream fires/h");
    for p in &sweep {
        let mark = if (p.threshold - prof).abs() < 1e-4 { "  <- profile" } else { "" };
        println!(
            " {:.2} | {:5.1}% ({:>4}/{:<4}) | {:>5}/{:<6}          | {}{}",
            p.threshold,
            p.recall * 100.0,
            p.pos_hits,
            p.pos_total,
            p.neg_clips_fired,
            p.neg_clips,
            fah(p),
            mark
        );
    }

    let groups = wake_eval::group_recall(&set, prof);
    if !groups.is_empty() {
        println!("\nRecall by condition at profile threshold {prof:.2}:");
        for g in &groups {
            println!(
                "  {:<16} {:5.1}% ({}/{})",
                g.group,
                100.0 * g.hits as f64 / g.total.max(1) as f64,
                g.hits,
                g.total
            );
        }
    }
    let mut neg_groups: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for s in set.iter().filter(|s| s.label == Label::Negative) {
        let e = neg_groups.entry(s.group.clone()).or_default();
        e.1 += 1;
        if !wake_eval::simulate_fires(&s.frames, prof).is_empty() {
            e.0 += 1;
        }
    }
    if !neg_groups.is_empty() {
        println!("\nNegative clips that fired, by group, at {prof:.2}:");
        for (g, (fired, n)) in &neg_groups {
            println!("  {g:<16} {fired}/{n}");
        }
    }

    let misses = wake_eval::misses(&set, prof);
    if !misses.is_empty() {
        println!("\nMissed positives (lowest peak first):");
        for m in misses.iter().take(15) {
            println!("  peak {:.3}  [{}] {}", m.peak_prob, m.group, m.path);
        }
    }
    let offenders = wake_eval::offenders(&set, prof, args.offenders);
    if !offenders.is_empty() {
        println!("\nFalse fires (hard-negative candidates):");
        for o in &offenders {
            println!("  score {:.3}  at {:>7.2}s  [{}] {}", o.score, o.time_s, o.group, o.path);
        }
    }
    match &recommended {
        Some(r) => println!(
            "\nBest threshold for <= {} stage-1 fires/h: {:.2} -> recall {:.1}%, {} fires/h",
            args.target_fah,
            r.threshold,
            r.recall * 100.0,
            fah(r).trim()
        ),
        None if at_prof.fa_per_hour.is_some() => {
            println!("\nNo threshold in the sweep meets <= {} fires/h.", args.target_fah)
        }
        None => println!("\nNo negative streams (> 30 s) scored, so no false fires per hour (record ambient audio)."),
    }
    if at_prof.neg_hours > 0.0 && at_prof.neg_hours < 1.0 {
        println!(
            "Note: only {:.2} h of negative streams; fires/h is not reliable below ~10 h.",
            at_prof.neg_hours
        );
    }

    if let Some(out) = &args.json {
        let report = Report {
            model,
            profile_threshold: prof,
            target_fa_per_hour: args.target_fah,
            positives: at_prof.pos_total,
            short_negative_clips: at_prof.neg_clips,
            negative_stream_hours: at_prof.neg_hours,
            skipped,
            at_profile_threshold: at_prof,
            recommended,
            sweep,
            group_recall: groups,
            negative_groups: neg_groups,
            misses,
            offenders,
            audio_hours_scored: audio_s / 3600.0,
            seconds,
        };
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(out, text).map_err(|e| format!("write {}: {e}", out.display()))?;
        println!("\nReport: {}", out.display());
    }
    Ok(())
}
