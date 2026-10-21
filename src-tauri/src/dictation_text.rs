//! Dictation text cleanup: raw speech-to-text in, text worth typing out.
//!
//! Pure, no I/O, no clocks. Everything the user says while dictating goes through [`clean`] before it
//! is typed. Rules are deliberately conservative: when in doubt the words are kept as spoken, because
//! silently rewriting what someone dictated is worse than leaving a filler word in.
//!
//! What it does: keeps the punctuation STT already produced, drops um/uh fillers, turns spoken
//! punctuation and layout words ("comma", "question mark", "new line", "new paragraph", quotes,
//! brackets) into the real characters, applies "scratch that" (alone = undo the previous insertion;
//! inside an utterance = drop what came before it) and an explicitly paused "no, wait" correction,
//! capitalises sentences and the pronoun "I", turns spoken numbers before units into digits
//! ("five p m" -> "5 pm"), strips the trailing period for one-line targets, and applies a user
//! dictionary.

/// Where the text is going and what came before it.
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    /// Nothing typed yet in this dictation, or the previous insertion ended a sentence / line.
    pub at_sentence_start: bool,
    /// One-line target (chat box, search field, address bar): no sentence-final period, no line breaks.
    pub single_line: bool,
    /// User dictionary: (spoken, written), matched as whole words, case-insensitive.
    pub dictionary: Vec<(String, String)>,
}

/// Result of cleaning one utterance.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cleaned {
    /// Text to insert (may contain `\n`). Empty when the utterance was only a command.
    pub text: String,
    /// "scratch that" with nothing else: remove the previous insertion.
    pub scratch_previous: bool,
}

const FILLERS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "er", "erm", "hmm", "mm", "mmm"];
const PUNCT_CHARS: &[char] = &[',', '.', '?', '!', ':', ';'];

/// STT punctuation is carried through as its own one-character "word".
fn is_marker(w: &str) -> bool {
    w.len() == 1 && w.chars().next().map_or(false, |c| PUNCT_CHARS.contains(&c))
}

/// Lowercase words; trailing `, . ? ! : ;` are split off as marker words, other edge punctuation dropped.
fn words_of(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tok in raw.split_whitespace() {
        let mut core = tok.trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '$' || c == '%'));
        let mut trailing: Vec<char> = Vec::new();
        while let Some(last) = core.chars().last() {
            if PUNCT_CHARS.contains(&last) {
                trailing.push(last);
                core = &core[..core.len() - last.len_utf8()];
            } else if last.is_alphanumeric() || last == '%' || last == '\'' {
                break;
            } else {
                core = &core[..core.len() - last.len_utf8()]; // quotes, brackets, dashes
            }
        }
        let core = core.trim_matches('\'');
        if !core.is_empty() {
            out.push(core.to_lowercase());
        }
        for c in trailing.into_iter().rev() {
            out.push(c.to_string());
        }
    }
    out
}

/// Words without punctuation markers (for phrase matching).
fn plain(words: &[String]) -> Vec<&str> {
    words.iter().filter(|w| !is_marker(w)).map(|w| w.as_str()).collect()
}

fn is_scratch_phrase(words: &[String]) -> bool {
    matches!(
        plain(words).join(" ").as_str(),
        "scratch that" | "delete that" | "undo that" | "remove that" | "strike that" | "never mind that"
    )
}

/// One token of the working stream.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    /// Punctuation glued to the previous word.
    Punct(&'static str),
    /// Opening bracket/quote: glued to the NEXT word.
    Open(&'static str),
    /// Closing bracket/quote: glued to the previous word.
    Close(&'static str),
    /// Line break(s).
    Break(&'static str),
    /// New line + "- ".
    Bullet,
}

const SCRATCH_MID: &[&[&str]] = &[&["scratch", "that"], &["delete", "that"], &["strike", "that"]];

fn starts_with_phrase(w: &[String], i: usize, phrase: &[&str]) -> bool {
    i + phrase.len() <= w.len() && phrase.iter().enumerate().all(|(k, p)| w[i + k] == *p)
}

/// Everything after `from` is punctuation markers (or nothing).
fn only_markers_after(w: &[String], from: usize) -> bool {
    w.iter().skip(from).all(|x| is_marker(x))
}

fn marker_static(w: &str) -> &'static str {
    match w {
        "," => ",",
        "." => ".",
        "?" => "?",
        "!" => "!",
        ":" => ":",
        _ => ";",
    }
}

/// Spoken punctuation / layout words -> tokens.
fn punctuation_at(w: &[String], i: usize) -> Option<(Tok, usize)> {
    let at = |p: &[&str]| starts_with_phrase(w, i, p);
    let before_break = |len: usize| {
        let j = {
            let mut j = i + len;
            while j < w.len() && is_marker(&w[j]) {
                j += 1;
            }
            j
        };
        j < w.len() && (starts_with_phrase(w, j, &["new", "line"]) || starts_with_phrase(w, j, &["new", "paragraph"]))
    };
    if at(&["comma"]) {
        return Some((Tok::Punct(","), 1));
    }
    if at(&["full", "stop"]) {
        return Some((Tok::Punct("."), 2));
    }
    // "period"/"colon" are also ordinary nouns ("a period of time"): punctuation only at the end of
    // the utterance or right before a line break.
    if at(&["period"]) && (only_markers_after(w, i + 1) || before_break(1)) {
        return Some((Tok::Punct("."), 1));
    }
    if at(&["colon"]) && (only_markers_after(w, i + 1) || before_break(1)) {
        return Some((Tok::Punct(":"), 1));
    }
    if at(&["question", "mark"]) {
        return Some((Tok::Punct("?"), 2));
    }
    if at(&["exclamation", "mark"]) || at(&["exclamation", "point"]) {
        return Some((Tok::Punct("!"), 2));
    }
    if at(&["semicolon"]) {
        return Some((Tok::Punct(";"), 1));
    }
    if at(&["semi", "colon"]) {
        return Some((Tok::Punct(";"), 2));
    }
    if at(&["new", "paragraph"]) {
        return Some((Tok::Break("\n\n"), 2));
    }
    if at(&["new", "line"]) || at(&["next", "line"]) {
        return Some((Tok::Break("\n"), 2));
    }
    if at(&["bullet", "point"]) {
        return Some((Tok::Bullet, 2));
    }
    if at(&["open", "quote"]) || at(&["begin", "quote"]) {
        return Some((Tok::Open("\""), 2));
    }
    if at(&["close", "quote"]) || at(&["end", "quote"]) {
        return Some((Tok::Close("\""), 2));
    }
    if at(&["unquote"]) {
        return Some((Tok::Close("\""), 1));
    }
    if at(&["open", "bracket"]) || at(&["open", "parenthesis"]) || at(&["open", "paren"]) {
        return Some((Tok::Open("("), 2));
    }
    if at(&["close", "bracket"]) || at(&["close", "parenthesis"]) || at(&["close", "paren"]) {
        return Some((Tok::Close(")"), 2));
    }
    None
}

// ── numbers ────────────────────────────────────────────────────────────────────────────────

fn small_number(word: &str) -> Option<u32> {
    Some(match word {
        "zero" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        _ => return None,
    })
}

fn tens(word: &str) -> Option<u32> {
    Some(match word {
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

/// 0..=99 starting at `i`: (value, words used).
fn number_at(w: &[String], i: usize) -> Option<(u32, usize)> {
    let first = w.get(i)?;
    if let Some(t) = tens(first) {
        if let Some(n) = w.get(i + 1).and_then(|x| small_number(x)).filter(|n| (1..=9).contains(n)) {
            return Some((t + n, 2));
        }
        return Some((t, 1));
    }
    small_number(first).map(|n| (n, 1))
}

/// Unit words that make a number a quantity worth writing as digits: (written unit, words used).
fn unit_after(w: &[String], i: usize) -> Option<(String, usize)> {
    let a = w.get(i)?.as_str();
    match a {
        "percent" => Some(("%".to_string(), 1)),
        "pm" | "am" => Some((a.to_string(), 1)),
        "p" | "a" if w.get(i + 1).map(|x| x.as_str()) == Some("m") => Some((format!("{a}m"), 2)),
        "dollars" | "dollar" => Some(("dollars".to_string(), 1)),
        "rupees" | "rupee" => Some(("rupees".to_string(), 1)),
        "o'clock" => Some(("o'clock".to_string(), 1)),
        _ => None,
    }
}

// ── the cleaner ─────────────────────────────────────────────────────────────────────────────

pub fn clean(raw: &str, ctx: &Ctx) -> Cleaned {
    let raw = raw.trim();
    if raw.is_empty() {
        return Cleaned::default();
    }
    let lower_raw = raw.to_lowercase();
    // A correction only counts when the speaker audibly paused around it ("send it to Bob, no, wait,
    // send it to Alice"); "no wait time" in a normal sentence must stay.
    let paused_correction = lower_raw.contains("no, wait")
        || lower_raw.contains(", no wait")
        || lower_raw.contains("wait, no")
        || lower_raw.contains("sorry, i mean")
        || lower_raw.contains("sorry i mean");

    let mut words = words_of(raw);
    // "Nexus, scratch that." — a leading address is not dictation.
    while words.first().map_or(false, |w| w == "nexus") && plain(&words).len() > 1 {
        words.remove(0);
        while words.first().map_or(false, |w| is_marker(w)) {
            words.remove(0);
        }
    }
    if plain(&words).is_empty() {
        return Cleaned::default();
    }
    if is_scratch_phrase(&words) {
        return Cleaned { text: String::new(), scratch_previous: true };
    }

    // 1. Mid-utterance backtracking: keep only what follows the LAST "scratch that".
    let mut start = 0;
    for i in 0..words.len() {
        if let Some(p) = SCRATCH_MID.iter().find(|p| starts_with_phrase(&words, i, p)) {
            start = i + p.len();
        }
    }
    let mut words: Vec<String> = words[start..].to_vec();
    while words.first().map_or(false, |w| is_marker(w)) {
        words.remove(0);
    }
    if plain(&words).is_empty() {
        return Cleaned::default();
    }

    // 2. Paused correction: drop the clause before the marker (back to the previous comma/period).
    if paused_correction {
        // "no, wait" arrives as ["no", ",", "wait"]: fold the pause into the phrase so it matches.
        let mut folded: Vec<String> = Vec::with_capacity(words.len());
        let mut k = 0;
        while k < words.len() {
            let is_pair = |a: &str, b: &str| words[k] == a && k + 2 < words.len() + 0 && is_marker(&words[k + 1]) && words[k + 2] == b;
            if is_pair("no", "wait") || is_pair("wait", "no") {
                folded.push(words[k].clone());
                folded.push(words[k + 2].clone());
                k += 3;
            } else if words[k] == "sorry" && k + 3 < words.len() && is_marker(&words[k + 1]) && words[k + 2] == "i" && words[k + 3] == "mean" {
                folded.extend(["sorry".to_string(), "i".to_string(), "mean".to_string()]);
                k += 4;
            } else {
                folded.push(words[k].clone());
                k += 1;
            }
        }
        words = folded;
        let markers: &[&[&str]] = &[&["no", "wait"], &["wait", "no"], &["sorry", "i", "mean"]];
        let mut i = 0;
        while i < words.len() {
            if let Some(p) = markers.iter().find(|p| starts_with_phrase(&words, i, p)) {
                let mut from = i;
                // skip the pause marker directly before the marker words ("bob, no wait")
                while from > 0 && is_marker(&words[from - 1]) {
                    from -= 1;
                }
                // walk back over the clause being corrected, stopping at an earlier boundary
                while from > 0 && !is_marker(&words[from - 1]) && words[from - 1] != "comma" {
                    from -= 1;
                }
                let mut tail_from = i + p.len();
                while tail_from < words.len() && is_marker(&words[tail_from]) {
                    tail_from += 1;
                }
                let head: Vec<String> = words[..from].to_vec();
                let tail: Vec<String> = words[tail_from..].to_vec();
                words = [head, tail].concat();
                i = from;
                continue;
            }
            i += 1;
        }
        if plain(&words).is_empty() {
            return Cleaned::default();
        }
    }

    // 3. Words -> tokens (markers, fillers, spoken punctuation, numbers, dictionary).
    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if is_marker(&words[i]) {
            toks.push(Tok::Punct(marker_static(&words[i])));
            i += 1;
            continue;
        }
        if let Some((tok, used)) = punctuation_at(&words, i) {
            toks.push(tok);
            i += used;
            continue;
        }
        let w = words[i].as_str();
        if FILLERS.contains(&w) {
            i += 1;
            continue;
        }
        if let Some((n, used)) = number_at(&words, i) {
            if let Some((unit, uused)) = unit_after(&words, i + used) {
                if unit == "%" {
                    toks.push(Tok::Word(format!("{n}%")));
                } else {
                    toks.push(Tok::Word(n.to_string()));
                    toks.push(Tok::Word(unit));
                }
                i += used + uused;
                continue;
            }
        }
        let written = ctx
            .dictionary
            .iter()
            .find(|(spoken, _)| spoken.eq_ignore_ascii_case(w))
            .map(|(_, written)| written.clone())
            .unwrap_or_else(|| w.to_string());
        toks.push(Tok::Word(written));
        i += 1;
    }

    // 4. Tokens -> text.
    let mut out = String::new();
    let mut glue_next = false; // after an opening bracket/quote or a line break: no space
    let mut capitalise = ctx.at_sentence_start;
    for t in &toks {
        match t {
            Tok::Word(word) => {
                if !out.is_empty() && !glue_next {
                    out.push(' ');
                }
                glue_next = false;
                out.push_str(&finish_word(word, capitalise));
                capitalise = false;
            }
            Tok::Punct(p) => {
                // hugs the previous word; a stray one at the start or after other punctuation is dropped
                let last = out.chars().last();
                if last.map_or(false, |c| !c.is_whitespace() && !PUNCT_CHARS.contains(&c) && c != '(') {
                    out.push_str(p);
                    if matches!(*p, "." | "?" | "!") {
                        capitalise = true;
                    }
                } else if last.map_or(false, |c| matches!(c, ',' | ':' | ';')) && matches!(*p, "." | "?" | "!") {
                    // "comma" then STT's own "." : the stronger mark wins
                    out.pop();
                    out.push_str(p);
                    capitalise = true;
                }
            }
            Tok::Open(o) => {
                if !out.is_empty() && !glue_next && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push_str(o);
                glue_next = true;
            }
            Tok::Close(c) => {
                out.push_str(c);
                glue_next = false;
            }
            Tok::Break(b) => {
                if ctx.single_line {
                    continue; // a one-line field cannot take a line break
                }
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push_str(b);
                glue_next = true;
                capitalise = true;
            }
            Tok::Bullet => {
                if ctx.single_line {
                    continue;
                }
                while out.ends_with(' ') {
                    out.pop();
                }
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("- ");
                glue_next = true;
            }
        }
    }

    // 5. One-line targets: STT's own trailing period is noise ("search for cats." -> "search for cats").
    if ctx.single_line {
        let t = out.trim_end();
        if t.ends_with('.') && !t.ends_with("..") {
            out = t[..t.len() - 1].to_string();
        }
    }
    Cleaned { text: out, scratch_previous: false }
}

/// Capitalise (sentence start / pronoun "I") and expand "i'm"-style contractions.
fn finish_word(word: &str, capitalise: bool) -> String {
    let mut w = word.to_string();
    if w == "i" {
        w = "I".to_string();
    } else if let Some(rest) = w.strip_prefix("i'") {
        w = format!("I'{rest}");
    }
    if capitalise {
        let mut c = w.chars();
        if let Some(f) = c.next() {
            w = f.to_uppercase().collect::<String>() + c.as_str();
        }
    }
    w
}

/// Does the inserted text end a sentence or line (the next utterance should start capitalised)?
pub fn ends_sentence(inserted: &str) -> bool {
    let t = inserted.trim_end_matches(' ');
    t.is_empty() || t.ends_with(['.', '?', '!', '\n'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Ctx {
        Ctx { at_sentence_start: true, single_line: false, dictionary: vec![] }
    }
    fn line() -> Ctx {
        Ctx { at_sentence_start: false, single_line: true, dictionary: vec![] }
    }
    fn t(raw: &str) -> String {
        clean(raw, &doc()).text
    }

    #[test]
    fn plain_text_is_capitalised_and_stt_punctuation_is_kept() {
        assert_eq!(t("hello this is a test"), "Hello this is a test");
        assert_eq!(t("Hello, this is a test."), "Hello, this is a test.");
        assert_eq!(t("hello comma this is a test full stop"), "Hello, this is a test.");
        // spoken AND recognised punctuation do not double up
        assert_eq!(t("hello comma, this is a test full stop."), "Hello, this is a test.");
    }

    #[test]
    fn fillers_are_dropped_but_real_words_stay() {
        assert_eq!(t("um hello uh world"), "Hello world");
        assert_eq!(t("so er I think we should go"), "So I think we should go");
        assert_eq!(t("the umbrella is here"), "The umbrella is here");
        assert_eq!(t("Um, hello."), "Hello.");
    }

    #[test]
    fn spoken_punctuation_becomes_characters() {
        assert_eq!(t("dear sam comma new line thanks for the update full stop"), "Dear sam,\nThanks for the update.");
        assert_eq!(t("are you coming question mark"), "Are you coming?");
        assert_eq!(t("that is great exclamation mark"), "That is great!");
        assert_eq!(t("note colon"), "Note:");
        assert_eq!(t("first part semicolon second part"), "First part; second part");
    }

    #[test]
    fn period_and_colon_are_only_punctuation_at_the_end() {
        assert_eq!(t("this is a period of time"), "This is a period of time");
        assert_eq!(t("that is all period"), "That is all.");
        assert_eq!(t("the colon is part of the body"), "The colon is part of the body");
    }

    #[test]
    fn layout_words() {
        assert_eq!(t("first line new paragraph second line"), "First line\n\nSecond line");
        assert_eq!(
            t("shopping list new line bullet point milk new line bullet point eggs"),
            "Shopping list\n- Milk\n- Eggs"
        );
        assert_eq!(clean("hello new line world", &line()).text, "hello world");
    }

    #[test]
    fn quotes_and_brackets_hug_their_content() {
        assert_eq!(t("he said open quote hello there close quote"), "He said \"hello there\"");
        assert_eq!(t("see open bracket figure two close bracket"), "See (figure two)");
    }

    #[test]
    fn capitalisation_continues_across_utterances() {
        let mid = Ctx { at_sentence_start: false, single_line: false, dictionary: vec![] };
        assert_eq!(clean("and then we left", &mid).text, "and then we left");
        let after_period = Ctx { at_sentence_start: true, ..mid.clone() };
        assert_eq!(clean("and then we left", &after_period).text, "And then we left");
        assert_eq!(t("i think i'm right and i'll prove it"), "I think I'm right and I'll prove it");
        assert_eq!(t("one thing full stop another thing"), "One thing. Another thing");
    }

    #[test]
    fn scratch_that_alone_undoes_the_previous_insertion() {
        for p in ["scratch that", "Scratch that.", "Nexus, scratch that.", "delete that", "undo that"] {
            let c = clean(p, &doc());
            assert!(c.scratch_previous && c.text.is_empty(), "{p}");
        }
        assert!(!clean("please do not scratch that surface", &doc()).scratch_previous);
    }

    #[test]
    fn scratch_that_inside_an_utterance_drops_what_came_before_it() {
        assert_eq!(t("meet me at five scratch that meet me at six"), "Meet me at six");
        assert_eq!(t("hello world scratch that"), "");
    }

    #[test]
    fn a_paused_correction_replaces_the_last_clause() {
        assert_eq!(t("send it to bob, no, wait, send it to alice"), "Send it to alice");
        assert_eq!(t("hello comma send it to bob, no wait, alice"), "Hello, alice");
        assert_eq!(t("see you tomorrow, sorry I mean on friday"), "On friday");
    }

    #[test]
    fn an_unpaused_no_wait_is_just_words() {
        assert_eq!(t("i have no wait time today"), "I have no wait time today");
        assert_eq!(t("there is no wait for the bus"), "There is no wait for the bus");
    }

    #[test]
    fn numbers_become_digits_only_before_units() {
        assert_eq!(t("meet at five p m"), "Meet at 5 pm");
        assert_eq!(t("it grew twenty five percent"), "It grew 25%");
        assert_eq!(t("about forty dollars"), "About 40 dollars");
        assert_eq!(t("i have five apples"), "I have five apples");
        assert_eq!(t("call at seven o'clock"), "Call at 7 o'clock");
    }

    #[test]
    fn one_line_targets_lose_the_trailing_period_only() {
        assert_eq!(clean("search for cats.", &line()).text, "search for cats");
        assert_eq!(clean("hello. how are you.", &line()).text, "hello. How are you");
        assert_eq!(clean("see you at 5", &line()).text, "see you at 5");
    }

    #[test]
    fn dictionary_replaces_whole_words_case_insensitively() {
        let ctx = Ctx {
            at_sentence_start: true,
            single_line: false,
            dictionary: vec![("zinc".into(), "Zync".into()), ("nexus".into(), "NEXUS".into())],
        };
        assert_eq!(clean("open the zinc repo", &ctx).text, "Open the Zync repo");
        assert_eq!(clean("zincing is not zinc", &ctx).text, "Zincing is not Zync");
    }

    #[test]
    fn cleaning_twice_does_not_change_the_words() {
        for s in ["Hello world", "Dear Sam,\nThanks.", "Meet at 5 pm", "He said \"hi\"", "one. two"] {
            let once = clean(s, &doc()).text;
            assert_eq!(words_of(&once), words_of(&clean(&once, &doc()).text), "{s}");
        }
    }

    #[test]
    fn empty_and_junk_never_panic() {
        for s in ["", "   ", ".", "comma", "new line", "scratch", "no wait", "um um um", "'", "🙂", ",,,", "scratch that scratch that"] {
            let _ = clean(s, &doc());
            let _ = clean(s, &line());
        }
    }

    #[test]
    fn ends_sentence_detects_the_continuation_state() {
        assert!(ends_sentence("Done."));
        assert!(ends_sentence("Line\n"));
        assert!(ends_sentence(""));
        assert!(!ends_sentence("and then"));
    }
}
