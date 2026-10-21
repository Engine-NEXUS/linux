//! Understands HOW a user asks for a control mode, not which exact words they use.
//!
//! "nexus i want to control the cursor" -> Hand mode (the USER drives the pointer)
//! "nexus i want to give command to you" / "take control of my laptop" -> Ghost mode (NEXUS drives)
//! "i want to control my laptop" -> ambiguous, so it asks instead of guessing.
//!
//! Pure and deterministic (works offline, < 1 ms). The exact phrases ("ghost mode", "hand mode", ...)
//! still match first in `intent_parser`; this runs after them and catches the many other ways people
//! say the same thing. The decision comes from three signals:
//!   1. WHO acts: "i want to / let me / can i" = the user; "you / nexus / i want you to" = NEXUS.
//!   2. WHAT is controlled: a pointer (cursor, mouse) or a whole machine (laptop, pc).
//!   3. HOW: hand cues (hand, finger, gesture, camera, ...) vs voice cues (voice, talk, command, ghost,
//!      "hands free", ...). The HOW cue wins over a bare imperative: "control the cursor with gestures"
//!      is the user's hand, "control my cursor for me" is NEXUS.
//! Questions about the feature ("how do I control the cursor with my hand") are NOT commands.

use crate::intent_parser::{ParseResult, ParsedIntent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ghost,
    Hand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Enter(Mode),
    Exit(Mode),
    /// Clearly wants to control something but it is unclear how: ask, do not guess.
    Ask,
}

const MAX_WORDS: usize = 18;
/// A decision needs this much evidence, and this much lead over the other mode.
const ENTER_MIN: i32 = 5;
const EXIT_MIN: i32 = 3;
const LEAD_MIN: i32 = 3;

const QUESTION_STARTS: &[&str] = &[
    "what", "why", "how", "who", "when", "where", "which", "whose", "explain", "describe", "is", "are",
    "does", "did", "was", "were",
];
const POINTER_NOUNS: &[&str] = &["cursor", "mouse", "pointer", "trackpad", "touchpad"];
const MACHINE_NOUNS: &[&str] = &["laptop", "computer", "pc", "screen", "desktop", "machine"];
const HAND_CUES: &[&str] = &[
    "hand", "hands", "finger", "fingers", "palm", "fist", "pinch", "gesture", "gestures", "camera",
    "webcam", "touchless", "midair", "mid-air", "wave", "waving",
];
const VOICE_CUES: &[&str] = &[
    "voice", "speak", "speaking", "talk", "talking", "say", "saying", "tell", "telling", "obey", "listen",
];
/// Verbs that mean "drive / operate". Deliberately NOT "take" or "run": "take the laptop to work" and
/// "run the program on my laptop" are not control requests (taking control has its own phrases).
const CONTROL_STEMS: &[&str] = &["control", "move", "drive", "steer", "operat", "use", "using", "handle", "navigat", "manag"];
const EXIT_CUES: &[&str] = &["stop", "quit", "exit", "leave", "end", "close", "cancel", "disable", "deactivate", "enough", "anymore", "off"];

const HAND_PHRASES: &[&str] = &[
    "hand mode", "hand control", "hand tracking", "hand mouse", "gesture control", "gesture mode", "camera mouse",
    "air mouse", "mouse in the air",
];
const TAKE_CONTROL_PHRASES: &[&str] = &[
    "take control", "take over", "taking control", "taking over", "take the mouse", "take my mouse", "take the cursor",
    "take my cursor", "take the controls",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

fn has(ws: &[String], list: &[&str]) -> bool {
    ws.iter().any(|w| list.contains(&w.as_str()))
}

fn count(ws: &[String], list: &[&str]) -> i32 {
    let mut seen: Vec<&str> = Vec::new();
    for w in ws {
        if let Some(x) = list.iter().find(|x| **x == w.as_str()) {
            if !seen.contains(x) {
                seen.push(x);
            }
        }
    }
    seen.len() as i32
}

fn has_stem(ws: &[String], stems: &[&str]) -> bool {
    ws.iter().any(|w| stems.iter().any(|s| w.starts_with(s)))
}

fn phrase(joined: &str, p: &str) -> bool {
    format!(" {joined} ").contains(&format!(" {p} "))
}

fn any_phrase(joined: &str, list: &[&str]) -> bool {
    list.iter().any(|p| phrase(joined, p))
}

/// Who is doing the controlling.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Agent {
    User,
    /// "you", "nexus", "i want you to", "take control": explicitly NEXUS.
    Nexus,
    /// A bare imperative ("control my cursor"): NEXUS unless a hand cue says the user is doing it.
    NexusWeak,
    Unknown,
}

fn agent(ws: &[String], joined: &str) -> Agent {
    let addresses_nexus = has(ws, &["you", "your", "yourself", "nexus"]);
    let user_frames = [
        "i want to", "i wanna", "i would like to", "i'd like to", "i need to", "let me", "lemme", "can i",
        "could i", "may i", "help me", "i'd love to", "i will", "i'll", "i wish to", "allow me to",
    ];
    let user_acts = any_phrase(joined, &user_frames) || (ws.first().map(|w| w == "i").unwrap_or(false) && !addresses_nexus);
    if any_phrase(joined, &["i want you", "i need you", "i'd like you", "i would like you"]) || any_phrase(joined, TAKE_CONTROL_PHRASES) {
        return Agent::Nexus;
    }
    if user_acts && !addresses_nexus {
        return Agent::User;
    }
    if addresses_nexus {
        return Agent::Nexus;
    }
    if ws.first().map(|w| ["control", "drive", "operate", "steer"].contains(&w.as_str())).unwrap_or(false) {
        return Agent::NexusWeak;
    }
    Agent::Unknown
}

#[derive(Debug, Default)]
struct Scores {
    ghost: i32,
    hand: i32,
}

fn score(ws: &[String], joined: &str, who: Agent) -> Scores {
    let mut s = Scores::default();
    let pointer = has(ws, POINTER_NOUNS);
    let machine = has(ws, MACHINE_NOUNS);
    let controls = has_stem(ws, CONTROL_STEMS) || any_phrase(joined, TAKE_CONTROL_PHRASES);

    // "hands free / no hands / without using my hands" is the opposite of a hand cue: it means voice.
    let hands_free = any_phrase(
        joined,
        &["hands free", "hands-free", "handsfree", "no hands", "without hands", "without my hands", "without using my hands", "without using hands"],
    );
    let mut hand_cues = count(ws, HAND_CUES);
    if hands_free {
        hand_cues = (hand_cues - 1).max(0); // the word "hands" there is not a hand cue
    }
    let hand_phrase = any_phrase(joined, HAND_PHRASES);
    let how_hand = hand_cues > 0 || hand_phrase;

    let voice_cues = count(ws, VOICE_CUES);
    let command_cue = has(ws, &["command", "commands", "order", "orders", "instruction", "instructions"]);
    let ghost_word = has(ws, &["ghost"]);
    let voice_phrase = any_phrase(joined, &["voice control", "do what i say", "do as i say", "listen to me", "listen to my", "give me my mouse back", "give me my cursor back", "give me my control back"]);
    let how_voice = hands_free || voice_cues > 0 || command_cue || ghost_word || voice_phrase;

    if hands_free {
        s.ghost += 5;
    }
    s.hand += 3 * hand_cues.min(2);
    if hand_phrase {
        s.hand += 5;
    }
    if any_phrase(joined, &["without touching", "without a mouse", "no mouse"]) {
        s.hand += 3;
    }
    if ghost_word {
        s.ghost += 6;
    }
    s.ghost += 3 * voice_cues.min(2);
    if command_cue {
        s.ghost += 3;
        // "give you commands", "give command to you"
        if has(ws, &["give", "giving", "send"]) && has(ws, &["you", "nexus"]) {
            s.ghost += 4;
        }
    }
    if voice_phrase {
        s.ghost += 5;
    }

    // Who + what. `weak` imperatives yield to a hand cue: "control the cursor with gestures".
    let who = if who == Agent::NexusWeak && how_hand { Agent::Unknown } else { who };
    let target = pointer || machine;
    if controls && target {
        match who {
            Agent::Nexus | Agent::NexusWeak => s.ghost += if pointer { 6 } else { 5 },
            Agent::User => {
                if hands_free || voice_cues > 0 || command_cue {
                    s.ghost += 5; // "i want to control the mouse with my voice"
                } else if how_hand {
                    s.hand += 5;
                } else if pointer {
                    s.hand += 6; // "i want to control the cursor": the user is the one driving it
                } else {
                    s.hand += 1; // "i want to control my laptop": by hand or by voice? undecided
                    s.ghost += 1;
                }
            }
            Agent::Unknown => {
                if how_hand {
                    s.hand += 3;
                }
                if how_voice {
                    s.ghost += 3;
                }
            }
        }
    }
    // NEXUS addressed + a voice cue + something to drive: "i want to tell you what to do on my laptop".
    if who == Agent::Nexus && (voice_cues > 0 || command_cue) && target && !controls {
        s.ghost += 3;
    }
    s
}

/// "nexus ...", "hey nexus, ..." is the wake word, not the sentence addressing NEXUS: drop it, or every
/// request would look like "you do it" and land in Ghost mode.
fn without_wake_prefix(mut ws: Vec<String>) -> Vec<String> {
    while ws.len() > 1 && ["nexus", "hey", "hi", "hello", "ok", "okay", "yo", "so", "and", "please"].contains(&ws[0].as_str()) {
        ws.remove(0);
    }
    ws
}

pub fn resolve(text: &str) -> Option<(Verdict, i32)> {
    let ws = without_wake_prefix(words(text));
    if ws.is_empty() || ws.len() > MAX_WORDS {
        return None;
    }
    if QUESTION_STARTS.contains(&ws[0].as_str()) {
        return None; // a question about the feature, not a command
    }
    let joined = ws.join(" ");
    let target = has(&ws, POINTER_NOUNS) || has(&ws, MACHINE_NOUNS);
    let domain = target
        || has(&ws, &["ghost", "gesture", "gestures", "webcam", "camera", "touchless", "command", "commands"])
        || any_phrase(&joined, HAND_PHRASES)
        || phrase(&joined, "voice control");
    if !domain {
        return None;
    }
    let exit = has(&ws, EXIT_CUES)
        || (phrase(&joined, "give me my") && phrase(&joined, "back"))
        || any_phrase(&joined, &["done with", "no more", "don't want", "dont want"]);
    let wants = has_stem(&ws, CONTROL_STEMS)
        || any_phrase(
            &joined,
            &[
                "i want", "let me", "can i", "could i", "may i", "i wanna", "i need", "start", "enable", "activate",
                "turn on", "switch to", "give", "begin", "launch", "please", "lemme", "i'd like", "i would like",
                "do what i say", "do as i say", "listen to",
            ],
        )
        || any_phrase(&joined, TAKE_CONTROL_PHRASES);
    if !wants && !exit {
        return None;
    }

    let who = agent(&ws, &joined);
    let s = score(&ws, &joined, who);
    let leader = if s.ghost >= s.hand { Mode::Ghost } else { Mode::Hand };
    let (lead, other) = if leader == Mode::Ghost { (s.ghost, s.hand) } else { (s.hand, s.ghost) };

    if exit {
        // Leaving a mode needs a clear signal for it ("stop" alone is handled elsewhere).
        return if lead >= EXIT_MIN && lead - other >= 2 { Some((Verdict::Exit(leader), lead)) } else { None };
    }
    if lead >= ENTER_MIN && lead - other >= LEAD_MIN {
        return Some((Verdict::Enter(leader), lead));
    }
    // Wants to drive the machine but how is unclear: ask once instead of guessing or chatting.
    if matches!(who, Agent::User | Agent::Unknown)
        && has_stem(&ws, &["control", "operat", "drive"])
        && target
        && lead - other < LEAD_MIN
        && s.ghost + s.hand >= 2
        && lead < ENTER_MIN
    {
        return Some((Verdict::Ask, lead));
    }
    None
}

pub const ASK_PROMPT: &str = "Do you want to control the cursor with your hand, or give me voice control of your computer? Say hand mode or ghost mode, sir.";

/// Entry point used by the intent parser (after the exact phrases).
pub fn parse(text: &str) -> Option<ParseResult> {
    let (verdict, _lead) = resolve(text)?;
    let intent = match verdict {
        Verdict::Enter(Mode::Hand) => ParsedIntent::EnterHandMode,
        Verdict::Exit(Mode::Hand) => ParsedIntent::ExitHandMode,
        Verdict::Enter(Mode::Ghost) => ParsedIntent::EnterGhostControl,
        Verdict::Exit(Mode::Ghost) => ParsedIntent::ExitGhostControl,
        Verdict::Ask => ParsedIntent::NeedMoreInfo { prompt: ASK_PROMPT.to_string() },
    };
    Some(ParseResult { intent, confidence: 0.88, source: "deterministic-mode-intent".to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(t: &str) -> Option<Verdict> {
        resolve(t).map(|(v, _)| v)
    }

    #[test]
    fn hand_mode_in_many_words() {
        for t in [
            "i want to control the cursor",
            "let me control the mouse with my hand",
            "i want to move the mouse with my hand",
            "i wanna move the cursor with my fingers",
            "use the camera to control the cursor",
            "control the cursor with gestures",
            "control the mouse with my hand",
            "can i use my hand as a mouse",
            "can i control the mouse with my hand",
            "turn on gesture control for the cursor",
            "enable hand tracking",
            "i want an air mouse",
            "i want to use my webcam as a mouse",
            "start touchless mouse control",
            "let me move the pointer with my palm",
            "i'd like to control my cursor using my camera",
            "move the cursor with my hand",
            "could i drive the mouse with gestures",
            "i want to control my laptop with my hand",
        ] {
            assert_eq!(v(t), Some(Verdict::Enter(Mode::Hand)), "{t}");
        }
    }

    #[test]
    fn ghost_mode_in_many_words() {
        for t in [
            "i want to give command to you",
            "i want to give you commands",
            "i want you to control my cursor",
            "take control of my laptop",
            "can you control my computer",
            "could you take the mouse",
            "take the mouse",
            "control my cursor for me",
            "you control the mouse",
            "i want to control my laptop with my voice",
            "i want to control the mouse by talking",
            "use my voice to control the pc",
            "i want hands free control of my computer",
            "listen to my commands and control the screen",
            "do what i say on my computer",
            "start voice control",
            "i want ghost mode",
            "activate ghost",
            "let me talk and you control the mouse",
            "i want to tell you what to do on my laptop",
        ] {
            assert_eq!(v(t), Some(Verdict::Enter(Mode::Ghost)), "{t}");
        }
    }

    #[test]
    fn leaving_a_mode_in_many_words() {
        for (t, m) in [
            ("stop controlling the mouse with my hand", Mode::Hand),
            ("turn off the gesture control", Mode::Hand),
            ("disable hand tracking", Mode::Hand),
            ("i'm done with the camera mouse", Mode::Hand),
            ("stop taking control of my mouse", Mode::Ghost),
            ("i don't want you to control my cursor anymore", Mode::Ghost),
            ("give me my mouse back", Mode::Ghost),
            ("stop the voice control", Mode::Ghost),
            ("leave ghost", Mode::Ghost),
        ] {
            assert_eq!(v(t), Some(Verdict::Exit(m)), "{t}");
        }
    }

    #[test]
    fn unclear_requests_ask_instead_of_guessing() {
        for t in ["i want to control my laptop", "i want to control the computer", "let me control my pc", "i wanna operate my computer"] {
            assert_eq!(v(t), Some(Verdict::Ask), "{t}");
        }
    }

    #[test]
    fn questions_and_unrelated_talk_are_not_commands() {
        for t in [
            "how do i control the cursor with my hand",
            "what is the best mouse to buy",
            "why is my mouse not working",
            "explain ghost mode",
            "is hand tracking accurate",
            "my hand hurts",
            "give me a hand with this code",
            "control panel",
            "i want to control the volume",
            "open the control panel",
            "the camera is on",
            "turn on the camera",
            "the mouse is broken",
            "move the window to the left",
            "move the cursor to the corner",
            "ghost mode is confusing",
            "i like the hand mode idea",
            "the computer is slow today",
            "i want a new laptop",
            "i want to use my laptop",
            "take the laptop to the office",
            "run the program on my laptop",
            "my laptop is running slow",
            "play some music on my computer",
            "use the mouse",
            "write a long email about the camera and the laptop and the mouse and the cursor and the meeting tomorrow",
        ] {
            assert_eq!(v(t), None, "{t}");
        }
    }

    #[test]
    fn who_acts_decides_between_the_modes() {
        // Same words, different subject.
        assert_eq!(v("i want to control the cursor"), Some(Verdict::Enter(Mode::Hand)));
        assert_eq!(v("i want you to control the cursor"), Some(Verdict::Enter(Mode::Ghost)));
        assert_eq!(v("can i control the mouse"), Some(Verdict::Enter(Mode::Hand)));
        assert_eq!(v("can you control the mouse"), Some(Verdict::Enter(Mode::Ghost)));
    }

    #[test]
    fn a_how_cue_beats_a_bare_imperative() {
        assert_eq!(v("control the cursor with gestures"), Some(Verdict::Enter(Mode::Hand)));
        assert_eq!(v("control my cursor for me"), Some(Verdict::Enter(Mode::Ghost)));
        assert_eq!(v("control the mouse with my voice"), Some(Verdict::Enter(Mode::Ghost)));
    }

    #[test]
    fn hands_free_means_voice_not_hands() {
        assert_eq!(v("i want hands free control of the mouse"), Some(Verdict::Enter(Mode::Ghost)));
        assert_eq!(v("control my pc without using my hands"), Some(Verdict::Enter(Mode::Ghost)));
    }

    #[test]
    fn the_wake_word_is_not_part_of_the_request() {
        for t in ["nexus i want to control the cursor", "hey nexus, let me control the mouse with my hand", "okay nexus can i use my hand as a mouse"] {
            assert_eq!(v(t), Some(Verdict::Enter(Mode::Hand)), "{t}");
        }
        for t in ["nexus take the mouse", "hey nexus i want to give you commands"] {
            assert_eq!(v(t), Some(Verdict::Enter(Mode::Ghost)), "{t}");
        }
        // A "nexus" later in the sentence still counts as addressing it.
        assert_eq!(v("i want nexus to control my cursor"), Some(Verdict::Enter(Mode::Ghost)));
    }

    #[test]
    fn it_maps_to_the_real_intents() {
        let r = parse("i want to control the cursor").unwrap();
        assert!(matches!(r.intent, ParsedIntent::EnterHandMode));
        assert!(r.source.starts_with("deterministic-") && !r.source.ends_with("-fuzzy"));
        assert!(matches!(parse("take the mouse").unwrap().intent, ParsedIntent::EnterGhostControl));
        assert!(matches!(parse("give me my mouse back").unwrap().intent, ParsedIntent::ExitGhostControl));
        assert!(matches!(parse("i want to control my laptop").unwrap().intent, ParsedIntent::NeedMoreInfo { .. }));
        assert!(parse("what is the weather").is_none());
    }
}
