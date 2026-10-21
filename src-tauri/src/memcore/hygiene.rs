//! Memory hygiene: what is worth remembering.
//!
//! An audit of the real stores (2026-10-10) found ~85-90 % of episodes and 24/24 conversation
//! turns were STT fragments ("then the", "from the", "Ssort."), hallucinations and
//! "Could you clarify..." replies, and the conversation brief's goal read "I don't know. I don't
//! know...". All of it was re-sent to the cloud on every turn. Nothing gated what entered memory.
//!
//! This module is the gate. Everything here is pure and unit-tested against lines taken from the
//! real files, so "is this worth remembering" is a table you can read, not a hunch.

/// Why a candidate was refused (for logs/tests; never shown to the user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    TooShort,
    TooLong,
    Repetitive,
    Hallucination,
    UnhelpfulReply,
    NotSpeech,
    /// Only filler/function words ("I'm gonna say that") - nothing to remember.
    NoContent,
}

/// Lowercase word tokens (letters/digits; apostrophes split, so "I'm" -> "i", "m").
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// Words that carry no memorable content on their own.
const STOP: &[&str] = &[
    "i", "me", "my", "you", "your", "we", "us", "it", "its", "he", "she", "they", "is", "am", "are",
    "was", "were", "be", "the", "a", "an", "and", "or", "but", "so", "to", "of", "in", "on", "at",
    "for", "with", "that", "this", "then", "there", "here", "gonna", "wanna", "say", "said", "just",
    "like", "yeah", "yes", "no", "ok", "okay", "oh", "um", "uh", "hmm", "well", "now", "not", "do",
    "did", "does", "don", "t", "s", "m", "ll", "re", "ve", "d", "what", "when", "how", "why", "who",
    "from", "up", "out", "going", "got", "get", "let", "thing", "things", "really", "very", "too",
];

/// Minimum content words for a transcript to be worth keeping.
pub const MIN_WORDS: usize = 3;
/// Beyond this a "transcript" is a ramble or a hallucination loop.
pub const MAX_WORDS: usize = 45;

/// Is this transcript real, usable speech? Pure.
///
/// Rejects fragments (< 3 words), rambles (> 45), repetition loops, STT hallucinations (the same
/// filter the STT path uses) and text with no real words at all.
pub fn transcript_quality(text: &str) -> Result<(), Reject> {
    let t = text.trim();
    if t.is_empty() {
        return Err(Reject::TooShort);
    }
    if crate::stt::looks_hallucinated(t) {
        return Err(Reject::Hallucination);
    }
    let w = words(t);
    // Tokens that carry meaning: 2+ letters (the "m" of "I'm" and stray "s" do not count).
    let content: Vec<&String> = w.iter().filter(|x| x.chars().count() >= 2).collect();
    if content.len() < MIN_WORDS {
        return Err(Reject::TooShort);
    }
    if w.len() > MAX_WORDS {
        return Err(Reject::TooLong);
    }
    let mut uniq: Vec<&String> = content.clone();
    uniq.sort();
    uniq.dedup();
    // "i don't know i don't know i don't know": few distinct words for the length.
    if content.len() >= 6 && (uniq.len() as f32) / (content.len() as f32) < 0.5 {
        return Err(Reject::Repetitive);
    }
    if !t.chars().any(|c| c.is_alphabetic()) {
        return Err(Reject::NotSpeech);
    }
    // At least one word that actually carries content (3+ letters, not filler).
    if !w.iter().any(|x| x.chars().count() >= 3 && !STOP.contains(&x.as_str())) {
        return Err(Reject::NoContent);
    }
    Ok(())
}

/// Replies that mean "nothing useful happened" (clarify/error/quota). A turn that ended like this
/// teaches nothing and must not become a memory.
pub fn reply_is_unhelpful(reply: &str) -> bool {
    let r = reply.trim().to_lowercase();
    if r.is_empty() {
        return true;
    }
    const STARTS: &[&str] = &[
        "could you clarify",
        "could you please clarify",
        "can you clarify",
        "i couldn't process",
        "i could not process",
        "i can't process",
        "i don't know",
        "i do not know",
        "i'm not sure what you",
        "i am not sure what you",
        "i'm sorry, i",
        "sorry, i",
        "sorry,",
        "i didn't catch",
        "didn't catch that",
        "i didn't understand",
        "didn't understand",
        "i couldn't",
        "i'm unable",
        "i am unable",
        "please try again",
        "i'm having trouble",
    ];
    if STARTS.iter().any(|s| r.starts_with(s)) {
        return true;
    }
    const CONTAINS: &[&str] = &[
        "rate limit",
        "quota",
        "try again later",
        "something went wrong",
        "could you tell me what you'd like",
        "could you rephrase",
        "not sure what you mean",
    ];
    CONTAINS.iter().any(|s| r.contains(s))
}

/// Should this completed turn become an episode (and feed fact mining)?
pub fn admit_episode(transcript: &str, response: &str) -> Result<(), Reject> {
    transcript_quality(transcript)?;
    if reply_is_unhelpful(response) {
        return Err(Reject::UnhelpfulReply);
    }
    Ok(())
}

/// Should this turn be recorded in the conversation thread?
/// Failed/clarification turns are kept (they power "I couldn't finish X") but ONLY when the
/// user's words were real; completed turns also need a useful reply.
pub fn admit_conversation_turn(transcript: &str, response: &str, completed: bool) -> Result<(), Reject> {
    transcript_quality(transcript)?;
    if completed && reply_is_unhelpful(response) {
        return Err(Reject::UnhelpfulReply);
    }
    Ok(())
}

/// A stored brief line (goal/active task/key point) is only worth sending if it is itself quality text.
pub fn brief_line_ok(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || (transcript_quality(t).is_ok() && !reply_is_unhelpful(t))
}

const CLEANUP_FLAG: &str = "hygiene_v1_done";

/// One-time (per store) cleanup of the junk that accumulated before the admit gate existed:
/// legacy `episodes.jsonl`, `conversation.jsonl` + its brief, and the episode tier of the sealed
/// store. Originals are copied to `memory/_backup_hygiene_v1/` first. Safe to call every start.
pub fn run_startup_cleanup(app_data_dir: &std::path::Path) {
    let already = super::with_store(app_data_dir, |s| s.meta_get(CLEANUP_FLAG).is_some()).unwrap_or(false);
    if already {
        return;
    }
    let mem = crate::memory::resolve_memory_dir(app_data_dir);
    let backup = mem.join("_backup_hygiene_v1");
    let _ = std::fs::create_dir_all(&backup);
    for (src, name) in [
        (mem.join("episodes.jsonl"), "episodes.jsonl"),
        (app_data_dir.join(crate::conversation::CONVERSATION_FILE), "conversation.jsonl"),
        (app_data_dir.join(crate::conversation::CONVERSATION_BRIEF_FILE), "conversation_brief.json"),
    ] {
        if src.exists() {
            let _ = std::fs::copy(&src, backup.join(name));
        }
    }
    let legacy = crate::memory::purge_junk_episodes(app_data_dir);
    let (turns, brief) = crate::conversation::purge_junk(app_data_dir);
    let sealed = super::with_store(app_data_dir, |s| {
        let n = s.purge_episodes_where(
            |v| transcript_quality(v).is_ok(),
            "hygiene",
            chrono::Utc::now().timestamp(),
        );
        s.meta_set(CLEANUP_FLAG, "1");
        n
    })
    .unwrap_or(0);
    tracing::info!(
        "[MEMORY] hygiene v1: removed {legacy} legacy episodes, {turns} conversation turns          (brief deleted: {brief}), {sealed} sealed episodes; backup in {}",
        backup.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real junk lines from the user's episodes.jsonl / conversation.jsonl (2026-10-10).
    const REAL_JUNK: &[&str] = &[
        "then the",
        "left.",
        "from the",
        "Ssort.",
        "Sermy.",
        "I'm gonna say that.",
        "Zinc.",
        "Thank you for watching",
        "Thank you.",
        "you",
        "I don't know. I don't know. I don't know. I don't know. I don't know. I don't know.",
        "openbread.",
        "love us.",
    ];

    /// Real commands/questions that must survive.
    const REAL_SIGNAL: &[&str] = &[
        "analyse my screen",
        "open my chat with mummy in whatsapp",
        "what is the weather like today in hyderabad",
        "remind me to call mom tomorrow at five",
        "search for the best protein sources for vegetarians",
        "I work at Google as a software engineer",
    ];

    #[test]
    fn real_junk_is_rejected() {
        for j in REAL_JUNK {
            assert!(
                transcript_quality(j).is_err(),
                "junk was admitted: {j:?}"
            );
        }
    }

    #[test]
    fn real_signal_is_admitted() {
        for s in REAL_SIGNAL {
            assert_eq!(transcript_quality(s), Ok(()), "signal was rejected: {s:?}");
        }
    }

    #[test]
    fn fragments_and_rambles_have_distinct_reasons() {
        assert_eq!(transcript_quality("then the"), Err(Reject::TooShort));
        assert_eq!(transcript_quality(""), Err(Reject::TooShort));
        let ramble = "word ".repeat(50);
        assert!(matches!(
            transcript_quality(&ramble),
            Err(Reject::TooLong) | Err(Reject::Repetitive) | Err(Reject::Hallucination)
        ));
        // Filler only: grammatical, but there is nothing to remember.
        assert_eq!(transcript_quality("I'm gonna say that."), Err(Reject::NoContent));
        // Whole-phrase loops are caught by the STT hallucination rules first; either reason is fine.
        assert!(matches!(
            transcript_quality("i don't know i don't know i don't know i don't know"),
            Err(Reject::Repetitive) | Err(Reject::Hallucination)
        ));
        // Scattered repetition (not a clean loop) is caught by the distinct-word ratio.
        assert_eq!(
            transcript_quality("yes please yes thing please yes please yes thing yes please"),
            Err(Reject::Repetitive)
        );
    }

    #[test]
    fn unhelpful_replies_are_recognised() {
        for r in [
            "Could you clarify what you mean, sir?",
            "I couldn't process that request.",
            "I don't know.",
            "Sorry, I didn't get that.",
            "Could you tell me what you'd like to dive into next?",
            "Rate limit reached, try again later.",
            "",
        ] {
            assert!(reply_is_unhelpful(r), "should be unhelpful: {r:?}");
        }
        for r in [
            "The capital of France is Paris.",
            "It is 24 degrees and sunny in Hyderabad.",
            "Opened your chat with Mummy, sir.",
        ] {
            assert!(!reply_is_unhelpful(r), "should be helpful: {r:?}");
        }
    }

    #[test]
    fn episode_needs_real_speech_and_a_useful_reply() {
        assert_eq!(
            admit_episode("what is the capital of france", "The capital of France is Paris."),
            Ok(())
        );
        assert_eq!(
            admit_episode("what is the capital of france", "I couldn't process that request."),
            Err(Reject::UnhelpfulReply)
        );
        assert_eq!(admit_episode("then the", "Ok sir."), Err(Reject::TooShort));
    }

    #[test]
    fn failed_turns_are_kept_only_for_real_words() {
        // "I couldn't finish <real request>" in the boot briefing: keep the real request...
        assert_eq!(
            admit_conversation_turn("book a table for two tonight", "Booking failed, sir.", false),
            Ok(())
        );
        // ...but never a fragment that merely failed.
        assert!(admit_conversation_turn("Zinc.", "Could you clarify?", false).is_err());
        // A garbled-but-wordy command that only earned a clarify reply is not an episode.
        assert!(admit_episode("Ghost to Mulder.", "Could you clarify what you mean?").is_err());
        assert!(admit_episode("It goes to mode", "Could you tell me what you'd like to dive into next?").is_err());
        // A completed turn with a clarify reply is not a memory.
        assert!(
            admit_conversation_turn("tell me about the moon landing", "Could you clarify?", true).is_err()
        );
    }

    #[test]
    fn startup_cleanup_removes_junk_keeps_signal_and_runs_once() {
        let mut d = std::env::temp_dir();
        d.push(format!("nexus_hygiene_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mem = crate::memory::resolve_memory_dir(&d);
        std::fs::create_dir_all(&mem).unwrap();

        let ep = |t: &str, r: &str| serde_json::json!({"ts": 1, "transcript": t, "response": r}).to_string();
        std::fs::write(
            mem.join("episodes.jsonl"),
            [
                ep("then the", "Could you clarify?"),
                ep("what is the capital of france", "The capital of France is Paris."),
                ep("Ssort.", "I couldn't process that request."),
            ]
            .join("
")
                + "
",
        )
        .unwrap();
        std::fs::write(
            d.join(crate::conversation::CONVERSATION_BRIEF_FILE),
            r#"{"goal":"I don't know. I don't know. I don't know."}"#,
        )
        .unwrap();

        run_startup_cleanup(&d);

        let left = std::fs::read_to_string(mem.join("episodes.jsonl")).unwrap();
        assert_eq!(left.lines().count(), 1, "only the real turn survives: {left}");
        assert!(left.contains("capital of france"));
        assert!(
            !d.join(crate::conversation::CONVERSATION_BRIEF_FILE).exists(),
            "poisoned brief must be deleted"
        );
        assert!(
            mem.join("_backup_hygiene_v1").join("episodes.jsonl").exists(),
            "originals are backed up first"
        );

        // Idempotent: a second start must not touch anything again.
        std::fs::write(mem.join("episodes.jsonl"), ep("then the", "x") + "
").unwrap();
        run_startup_cleanup(&d);
        assert_eq!(
            std::fs::read_to_string(mem.join("episodes.jsonl")).unwrap().lines().count(),
            1,
            "cleanup is one-time"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn poisoned_brief_lines_are_skipped() {
        assert!(!brief_line_ok(
            "I don't know. I don't know. I don't know. You're not going to be in the middle of the night"
        ));
        assert!(brief_line_ok("analyse the pull request for the servx repository"));
        assert!(brief_line_ok(""));
    }
}
