//! How-to questions: the steps belong in the sidebar, never in the speaker.
//!
//! Live log 2026-10-17: "shut down my laptop" was misheard, fell through to the cloud model, and NEXUS
//! read a 288-character tutorial aloud for ~18 s. Two rules came out of it:
//! 1. A *question* about power ("how do I shut down my laptop") is answered locally and instantly with a
//!    steps card in the sidebar plus one short spoken line (no cloud call, works offline, cannot invent steps).
//! 2. Any other how-to *answer* that is long or step-shaped is shown in the sidebar and announced in one
//!    short line instead of being read out. Ordinary short conversation is still spoken (speed first).
//!
//! Everything here is pure and unit-tested; `orchestrator.rs` has two narrow hooks.

use crate::power_cmd::PowerKind;

/// The one line that is spoken when the steps went to the sidebar.
pub const SIDEBAR_SPOKEN: &str = "The steps are in the sidebar, sir.";

/// An answer at or below this many characters is spoken even if it is a how-to (short = fast).
const SPEAK_MAX_CHARS: usize = 160;

const POLITE_PREFIXES: &[&str] = &["nexus ", "hey nexus ", "please ", "can you ", "could you ", "would you ", "tell me "];

/// Sentence openers that mark a how-to question. The remainder is the topic.
const HOWTO_LEADS: &[&str] = &[
    "how do i ",
    "how can i ",
    "how do you ",
    "how would i ",
    "how should i ",
    "how do we ",
    "how to ",
    "show me how to ",
    "teach me how to ",
    "explain how to ",
    "explain to me how to ",
    "tell me how to ",
    "what are the steps to ",
    "what are the steps for ",
    "what is the way to ",
    "what's the way to ",
    "whats the way to ",
    "where do i click to ",
    "where do i go to ",
    "steps to ",
    "steps for ",
    "the steps to ",
    "instructions to ",
    "instructions for ",
    "guide me to ",
];

/// Words that make a long answer a procedure ("press ... then select ...").
const STEP_VERBS: &[&str] = &[
    "press", "click", "select", "choose", "tap", "hold", "navigate", "scroll", "drag", "type", "enter", "open",
];

fn normalise(text: &str) -> String {
    let mut t = text.replace(',', " ").to_lowercase();
    t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    t = t.trim_matches(|c: char| matches!(c, '.' | '!' | '?' | ' ')).to_string();
    loop {
        let before = t.clone();
        for p in POLITE_PREFIXES {
            if let Some(rest) = t.strip_prefix(p) {
                t = rest.trim().to_string();
            }
        }
        if t == before {
            return t;
        }
    }
}

/// `Some(topic)` when the sentence is a how-to question ("how do I shut down my laptop" -> "shut down my laptop").
pub fn howto_topic(text: &str) -> Option<String> {
    let t = normalise(text);
    for lead in HOWTO_LEADS {
        if let Some(rest) = t.strip_prefix(lead) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// A ready-to-show steps card.
pub struct StepsCard {
    pub title: &'static str,
    pub markdown: String,
}

fn power_card(kind: PowerKind) -> StepsCard {
    match kind {
        PowerKind::Shutdown => StepsCard {
            title: "How to shut down your laptop",
            markdown: "# Shut down your laptop (Windows)\n\n\
1. **Start menu**: click Start, then the power icon, then **Shut down**.\n\
2. **Keyboard**: on the desktop press **Alt + F4**, choose **Shut down**, press Enter.\n\
3. **Ctrl + Alt + Delete**: click the power icon in the corner, then **Shut down**.\n\
4. **Command line**: press **Win + R**, type `shutdown /s /t 0` and press Enter.\n\
5. **Last resort**: hold the power button for about 5 seconds (unsaved work is lost).\n\n\
Or just say **\"shut down my laptop\"** and I will do it for you."
                .to_string(),
        },
        PowerKind::Restart => StepsCard {
            title: "How to restart your laptop",
            markdown: "# Restart your laptop (Windows)\n\n\
1. **Start menu**: click Start, then the power icon, then **Restart**.\n\
2. **Keyboard**: press **Alt + F4** on the desktop, choose **Restart**, press Enter.\n\
3. **Ctrl + Alt + Delete**: click the power icon, then **Restart**.\n\
4. **Command line**: press **Win + R**, type `shutdown /r /t 0` and press Enter.\n\n\
Or just say **\"restart my laptop\"** and I will do it for you."
                .to_string(),
        },
        PowerKind::Sleep => StepsCard {
            title: "How to put your laptop to sleep",
            markdown: "# Put your laptop to sleep (Windows)\n\n\
1. **Start menu**: click Start, then the power icon, then **Sleep**.\n\
2. **Keyboard**: press **Alt + F4** on the desktop, choose **Sleep**, press Enter.\n\
3. **Lid**: close the lid (if your power settings say so).\n\n\
Or just say **\"put my laptop to sleep\"** and I will do it for you."
                .to_string(),
        },
        PowerKind::Lock => StepsCard {
            title: "How to lock your screen",
            markdown: "# Lock your screen (Windows)\n\n\
1. **Keyboard**: press **Win + L**.\n\
2. **Ctrl + Alt + Delete**: choose **Lock**.\n\
3. **Start menu**: click your profile picture, then **Lock**.\n\n\
Or just say **\"lock my laptop\"** and I will do it for you."
                .to_string(),
        },
        PowerKind::SignOut => StepsCard {
            title: "How to sign out of Windows",
            markdown: "# Sign out of Windows\n\n\
1. **Start menu**: click your profile picture, then **Sign out**.\n\
2. **Ctrl + Alt + Delete**: choose **Sign out**.\n\
3. **Command line**: press **Win + R**, type `logoff` and press Enter.\n\n\
Or just say **\"sign me out\"** and I will do it for you."
                .to_string(),
        },
    }
}

/// A how-to question about a power action ("how do I shut down my laptop"): answered locally, no cloud.
pub fn static_card(text: &str) -> Option<StepsCard> {
    let topic = howto_topic(text)?;
    crate::power_cmd::parse(&topic).map(power_card)
}

/// How a cloud answer reaches the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Read it out (short or conversational).
    Speak,
    /// Show it in the sidebar and say only `SIDEBAR_SPOKEN`.
    Sidebar,
}

/// A long text that reads like a procedure: three or more step verbs, or three numbered/bulleted lines.
pub fn looks_procedural(answer: &str) -> bool {
    let lower = answer.to_lowercase();
    let verbs = lower
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| STEP_VERBS.contains(w))
        .count();
    if verbs >= 3 {
        return true;
    }
    let list_lines = answer
        .lines()
        .map(str::trim_start)
        .filter(|l| {
            l.starts_with("- ")
                || l.starts_with("* ")
                || l.chars().next().map_or(false, |c| c.is_ascii_digit()) && l.contains(['.', ')'])
        })
        .count();
    list_lines >= 3
}

/// Speak or show? A how-to question or a procedure-shaped answer, longer than a short reply, goes to the
/// sidebar; everything else is spoken exactly as before.
pub fn delivery_for(transcript: &str, answer: &str) -> Delivery {
    if answer.chars().count() <= SPEAK_MAX_CHARS {
        return Delivery::Speak;
    }
    if howto_topic(transcript).is_some() || looks_procedural(answer) {
        Delivery::Sidebar
    } else {
        Delivery::Speak
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reply that was read aloud for ~18 s on 2026-10-17 (the model's wording, slightly shortened).
    const LOG_REPLY: &str = "If you want to shut down your laptop, press and hold the power button until the shutdown menu appears, then select \u{201c}Shut down,\u{201d} or on Windows you can press Ctrl + Alt + Delete and click the power icon, then choose Shut down. Let me know if you need more help.";

    #[test]
    fn howto_questions_are_detected_with_their_topic() {
        assert_eq!(howto_topic("How do I shut down my laptop?").as_deref(), Some("shut down my laptop"));
        assert_eq!(howto_topic("Nexus, how can I restart my pc").as_deref(), Some("restart my pc"));
        assert_eq!(howto_topic("can you show me how to lock my screen").as_deref(), Some("lock my screen"));
        assert_eq!(howto_topic("what are the steps to take a screenshot").as_deref(), Some("take a screenshot"));
        assert_eq!(howto_topic("how to turn off my computer").as_deref(), Some("turn off my computer"));
    }

    #[test]
    fn commands_and_chat_are_not_howtos() {
        for t in [
            "shut down my laptop",
            "open notepad",
            "what is the capital of france",
            "how are you",
            "how much is this",
            "how many repositories do i have",
            "who is the president",
            "tell me a joke",
            "what time is it",
            "",
        ] {
            assert_eq!(howto_topic(t), None, "{t:?}");
        }
    }

    #[test]
    fn power_howtos_get_a_local_card_and_nothing_else_does() {
        for (q, needle) in [
            ("how do I shut down my laptop", "shutdown /s /t 0"),
            ("how do i turn off my computer", "shutdown /s /t 0"),
            ("how can i restart my pc", "shutdown /r /t 0"),
            ("how do i put my laptop to sleep", "Sleep"),
            ("how do i lock my screen", "Win + L"),
            ("how do i sign out of windows", "logoff"),
            ("nexus how do i shut down my laptop please", "Alt + F4"),
        ] {
            let c = static_card(q).unwrap_or_else(|| panic!("no card for {q:?}"));
            assert!(c.markdown.contains(needle), "{q:?} card missing {needle:?}");
            assert!(c.markdown.starts_with("# "), "card is markdown with a heading");
        }
        assert!(static_card("how do i bake bread").is_none());
        assert!(static_card("shut down my laptop").is_none(), "a command is not a question");
        assert!(static_card("how do i shut down notepad").is_none(), "an app is not the computer");
    }

    #[test]
    fn the_shutdown_card_lists_every_method() {
        let c = static_card("how do i shut down my laptop").unwrap();
        for m in ["Start menu", "Alt + F4", "Ctrl + Alt + Delete", "shutdown /s /t 0", "power button"] {
            assert!(c.markdown.contains(m), "{m}");
        }
    }

    #[test]
    fn the_logged_leak_goes_to_the_sidebar() {
        assert!(LOG_REPLY.chars().count() > SPEAK_MAX_CHARS);
        assert!(looks_procedural(LOG_REPLY));
        // the transcript was a mishearing, not a how-to: the ANSWER shape alone must catch it
        assert_eq!(delivery_for("check down my laptop.", LOG_REPLY), Delivery::Sidebar);
    }

    #[test]
    fn short_and_conversational_answers_are_still_spoken() {
        assert_eq!(delivery_for("how do i shut down my laptop", "Use the Start menu, sir."), Delivery::Speak);
        let long_chat = "Paris is the capital of France and has been for a very long time. It is known for its museums, its food and the river that runs through the middle of the city, which makes it one of the most visited places in the world.";
        assert!(long_chat.chars().count() > SPEAK_MAX_CHARS);
        assert_eq!(delivery_for("what is the capital of france", long_chat), Delivery::Speak);
    }

    #[test]
    fn a_long_answer_to_a_howto_question_goes_to_the_sidebar() {
        let long = "To take a screenshot you use the Snipping Tool, which lets you pick a region, a window or the full screen, and it also lets you annotate the picture afterwards and save it wherever you like on the disk.";
        assert!(long.chars().count() > SPEAK_MAX_CHARS);
        assert_eq!(delivery_for("how do i take a screenshot", long), Delivery::Sidebar);
        assert_eq!(delivery_for("tell me about screenshots", long), Delivery::Speak);
    }


    /// The hooks live on the Unknown fallback, so these sentences must still reach it: the deterministic
    /// parser must NOT claim them as some other command (e.g. close-app "my laptop").
    #[test]
    fn mishearings_and_howtos_reach_the_unknown_fallback() {
        for t in [
            // unlogged variants: not strict commands, so they reach the fallback and get asked about
            "shock down my laptop.",
            "tick down my laptop.",
            "how do I shut down my laptop?",
            "how do i restart my pc",
        ] {
            let parsed = crate::intent_parser::parse_deterministic(t);
            assert!(
                parsed.is_none(),
                "{t:?} was claimed by the deterministic parser as {:?}",
                parsed.map(|p| crate::intent_parser::intent_to_label(&p.intent))
            );
        }
        // and the clean command still is a power action
        let p = crate::intent_parser::parse_deterministic("shut down my laptop").expect("power command");
        assert_eq!(crate::intent_parser::intent_to_label(&p.intent), "power_action");
    }

    #[test]
    fn numbered_lists_count_as_procedures() {
        let list = "1. Open the Settings app from the Start menu.\n2. Pick the System category on the left side.\n3. Scroll down to About and read the device name that is shown at the top of the page for you.";
        assert!(list.chars().count() > SPEAK_MAX_CHARS);
        assert!(looks_procedural(list));
        assert!(!looks_procedural("Paris is the capital of France."));
    }
}
