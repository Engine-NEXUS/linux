//! Optional name redaction for cloud turns (plan P2, setting
//! `memcoreRedactNames`, default off).
//!
//! When on, the names of people NEXUS knows locally (contacts, profile
//! people) are swapped for stable labels ("Person A", "Person B", …) in the
//! text that leaves the device, and swapped back in the reply and in what is
//! stored locally. The mapping is deterministic over the sorted roster, so
//! `unredact(redact(x)) == x` for any text that contains those names.
//!
//! Honest limits: only names NEXUS already knows are redacted — a stranger's
//! name typed in a sentence is not. Redaction is whole-word and
//! case-insensitive; the original spelling comes back as the roster spelled it.

use std::path::Path;

use regex::Regex;

/// `memcoreRedactNames` from settings.json (default false).
pub fn enabled(app_data_dir: &Path) -> bool {
    std::fs::read_to_string(app_data_dir.join("settings.json"))
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|j| j.get("memcoreRedactNames").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// Locally known people: contacts.json names + profile people, de-duplicated
/// case-insensitively and sorted, so label assignment is stable.
pub fn roster(app_data_dir: &Path) -> Vec<String> {
    let mut all = crate::memory::read_contact_names(app_data_dir);
    if let Some(p) = crate::memory::read_user_profile(app_data_dir) {
        all.extend(p.people);
    }
    normalize_roster(all)
}

pub fn normalize_roster(names: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeMap::<String, String>::new();
    for n in names {
        let n = n.trim().to_string();
        if n.chars().count() >= 3 {
            seen.entry(n.to_lowercase()).or_insert(n);
        }
    }
    seen.into_values().collect()
}

/// "Person A".."Person Z", then "Person 27", "Person 28", …
pub fn label(i: usize) -> String {
    if i < 26 {
        format!("Person {}", (b'A' + i as u8) as char)
    } else {
        format!("Person {}", i + 1)
    }
}

/// Surface forms of one name: the full name and (when multi-word) its first
/// word, longest first so "Asha Verma" is replaced before "Asha".
fn forms(name: &str) -> Vec<String> {
    let mut f = vec![name.to_string()];
    if let Some(first) = name.split_whitespace().next() {
        if first != name && first.chars().count() >= 3 {
            f.push(first.to_string());
        }
    }
    f
}

pub fn redact(text: &str, roster: &[String]) -> String {
    let mut pairs: Vec<(String, String)> = vec![];
    for (i, name) in roster.iter().enumerate() {
        for f in forms(name) {
            pairs.push((f, label(i)));
        }
    }
    // Longest first so a full name wins over its own first word.
    pairs.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));
    let mut out = text.to_string();
    for (form, lab) in pairs {
        if let Ok(re) = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(&form))) {
            out = re.replace_all(&out, lab.as_str()).into_owned();
        }
    }
    out
}

/// Put real names back. `Person A` → the roster's first form for index 0, …
pub fn unredact(text: &str, roster: &[String]) -> String {
    let Ok(re) = Regex::new(r"\bPerson ([A-Z]|\d+)\b") else { return text.to_string() };
    re.replace_all(text, |caps: &regex::Captures| {
        let tag = &caps[1];
        let idx = if tag.len() == 1 && tag.as_bytes()[0].is_ascii_uppercase() {
            Some((tag.as_bytes()[0] - b'A') as usize)
        } else {
            tag.parse::<usize>().ok().and_then(|n| n.checked_sub(1))
        };
        match idx.and_then(|i| roster.get(i)) {
            Some(name) => name.clone(),
            None => caps[0].to_string(),
        }
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster3() -> Vec<String> {
        normalize_roster(vec!["Mom".into(), "Asha Verma".into(), "rahul".into(), "Al".into()])
    }

    #[test]
    fn roster_is_sorted_deduped_and_drops_tiny_names() {
        assert_eq!(roster3(), vec!["Asha Verma", "Mom", "rahul"]);
        assert_eq!(normalize_roster(vec!["MOM".into(), "mom".into()]), vec!["MOM"]);
    }

    #[test]
    fn redacts_whole_words_case_insensitively_with_stable_labels() {
        let r = roster3();
        let out = redact("Message mom and ASHA about Momentum; tell Rahul.", &r);
        // Asha Verma = A (also by first name), Mom = B, rahul = C. "Momentum" untouched.
        assert_eq!(out, "Message Person B and Person A about Momentum; tell Person C.");
    }

    #[test]
    fn full_name_wins_over_first_name() {
        let r = roster3();
        assert_eq!(redact("Call Asha Verma now", &r), "Call Person A now");
    }

    #[test]
    fn unredact_restores_names_and_ignores_unknown_labels() {
        let r = roster3();
        assert_eq!(unredact("Person B says hi to Person C", &r), "Mom says hi to rahul");
        assert_eq!(unredact("Person Z and Person 99 are strangers", &r), "Person Z and Person 99 are strangers");
    }

    #[test]
    fn round_trip_for_text_using_roster_spelling() {
        let r = roster3();
        let text = "Tell Mom that Asha Verma and rahul are coming";
        assert_eq!(unredact(&redact(text, &r), &r), text);
    }

    #[test]
    fn labels_continue_past_z() {
        assert_eq!(label(0), "Person A");
        assert_eq!(label(25), "Person Z");
        assert_eq!(label(26), "Person 27");
    }

    #[test]
    fn empty_roster_is_a_no_op() {
        assert_eq!(redact("hello Mom", &[]), "hello Mom");
        assert_eq!(unredact("Person A", &[]), "Person A");
    }
}
