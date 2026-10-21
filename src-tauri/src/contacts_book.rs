//! The local contact book: name -> WhatsApp-ready phone number.
//!
//! Why: opening a specific person's WhatsApp chat by scraping the chat list can never be reliable
//! (2026-10-15 log: UI Automation could not read the list, 57 s, then "WhatsApp lost focus"). The one
//! deterministic way is the phone-number link (`whatsapp://send?phone=...`), and that needs the number.
//! This module gets the numbers from a contacts file the user exports once (`.vcf`, from the phone or
//! Google Contacts) into the SAME `contacts.json` (`{"Name": "number"}`) the app already reads
//! (`command_executor::whatsapp_chat`, `memory::read_contact_names`). Everything stays on the PC; numbers
//! are never put into a cloud prompt (the context pack redacts digits; this file is not part of it).
//!
//! Manually written entries always win over imported ones.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One contact read from a vCard.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub name: String,
    pub phones: Vec<String>,
}

/// vCard text -> cards. Handles folded lines, `FN`, `N` (fallback), `TEL` with parameters / groups.
pub fn parse_vcf(text: &str) -> Vec<Card> {
    // unfold: a line starting with a space or tab continues the previous one
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(raw.trim_start());
        } else {
            lines.push(raw.to_string());
        }
    }
    let mut out = Vec::new();
    let mut cur: Option<(String, String, Vec<String>)> = None; // (fn, n, phones)
    for l in lines {
        let up = l.trim().to_uppercase();
        if up == "BEGIN:VCARD" {
            cur = Some((String::new(), String::new(), Vec::new()));
            continue;
        }
        if up == "END:VCARD" {
            if let Some((f, n, phones)) = cur.take() {
                let name = if !f.trim().is_empty() { f.trim().to_string() } else { n.trim().to_string() };
                if !name.is_empty() && !phones.is_empty() {
                    out.push(Card { name, phones });
                }
            }
            continue;
        }
        let Some(c) = cur.as_mut() else { continue };
        let Some((key, value)) = l.split_once(':') else { continue };
        // "item1.TEL;type=CELL" -> "TEL"
        let key_up = key.to_uppercase();
        let prop = key_up.rsplit('.').next().unwrap_or("").split(';').next().unwrap_or("");
        match prop {
            "FN" => c.0 = value.trim().to_string(),
            "N" => {
                // family;given;additional;prefix;suffix
                let parts: Vec<&str> = value.split(';').collect();
                let given = parts.get(1).copied().unwrap_or("").trim();
                let family = parts.first().copied().unwrap_or("").trim();
                c.1 = format!("{given} {family}").trim().to_string();
            }
            "TEL" => {
                let v = value.trim();
                if !v.is_empty() {
                    c.2.push(v.to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// Common country calling codes (for guessing the user's own from the numbers that carry a `+`).
const COUNTRY_CODES: &[&str] = &[
    "1", "7", "20", "27", "30", "31", "32", "33", "34", "39", "40", "41", "44", "45", "46", "47", "48", "49", "52", "54",
    "55", "56", "57", "58", "60", "61", "62", "63", "64", "65", "66", "81", "82", "84", "86", "90", "91", "92", "93",
    "94", "95", "98", "212", "213", "234", "254", "255", "256", "351", "352", "353", "358", "380", "420", "880", "886",
    "960", "962", "965", "966", "971", "972", "973", "974", "977",
];

/// The calling code in `+<digits>`, longest known prefix first.
fn country_code_of(plus_digits: &str) -> Option<&'static str> {
    let mut best: Option<&'static str> = None;
    for cc in COUNTRY_CODES {
        if plus_digits.starts_with(cc) && best.map_or(true, |b| cc.len() > b.len()) {
            best = Some(cc);
        }
    }
    best
}

/// The user's own calling code: the most common one among numbers written with a leading `+`.
pub fn infer_default_cc(cards: &[Card]) -> Option<String> {
    let mut count: BTreeMap<&'static str, usize> = BTreeMap::new();
    for c in cards {
        for p in &c.phones {
            let t = p.trim();
            if let Some(rest) = t.strip_prefix('+') {
                let digits: String = rest.chars().filter(|d| d.is_ascii_digit()).collect();
                if let Some(cc) = country_code_of(&digits) {
                    *count.entry(cc).or_insert(0) += 1;
                }
            }
        }
    }
    count.into_iter().max_by_key(|(_, n)| *n).map(|(cc, _)| cc.to_string())
}

/// A number in the form WhatsApp links need (digits, with country code), or `None`.
pub fn whatsapp_number(raw: &str, default_cc: Option<&str>) -> Option<String> {
    let t = raw.trim();
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    let n = if t.starts_with('+') {
        digits
    } else if let Some(rest) = digits.strip_prefix("00") {
        rest.to_string()
    } else {
        let cc = default_cc?;
        let local = digits.trim_start_matches('0');
        if digits.starts_with(cc) && local.len() >= 11 {
            local.to_string() // already carries the country code, just without the plus
        } else {
            format!("{cc}{local}")
        }
    };
    (8..=15).contains(&n.len()).then_some(n)
}

/// What an import changed.
#[derive(Debug, Default, PartialEq)]
pub struct ImportReport {
    pub added: usize,
    pub already_known: usize,
    pub no_usable_number: usize,
}

fn contacts_path(app_data: &Path) -> PathBuf {
    app_data.join("contacts.json")
}

/// Read `contacts.json` (`{"Name": "number"}`); missing or corrupt = empty.
pub fn load(app_data: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(contacts_path(app_data))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| {
            v.as_object().map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
        })
        .unwrap_or_default()
}

fn save(app_data: &Path, map: &BTreeMap<String, String>) -> std::io::Result<()> {
    let tmp = contacts_path(app_data).with_extension("json.tmp");
    std::fs::create_dir_all(app_data)?;
    std::fs::write(&tmp, serde_json::to_vec_pretty(map).unwrap_or_default())?;
    std::fs::rename(tmp, contacts_path(app_data))
}

/// Merge cards into `contacts.json`. Existing names (any case) are kept as they are.
pub fn import_cards(app_data: &Path, cards: &[Card]) -> std::io::Result<ImportReport> {
    let cc = infer_default_cc(cards);
    let mut map = load(app_data);
    let mut known: std::collections::HashSet<String> = map.keys().map(|k| k.to_lowercase()).collect();
    let mut report = ImportReport::default();
    for card in cards {
        // Numbers written with a `+` carry their own country code: prefer them over local-format ones.
        let ordered = card
            .phones
            .iter()
            .filter(|p| p.trim().starts_with('+'))
            .chain(card.phones.iter().filter(|p| !p.trim().starts_with('+')));
        let Some(number) = ordered.into_iter().find_map(|p| whatsapp_number(p, cc.as_deref())) else {
            report.no_usable_number += 1;
            continue;
        };
        if known.contains(&card.name.to_lowercase()) {
            report.already_known += 1;
            continue;
        }
        known.insert(card.name.to_lowercase());
        map.insert(card.name.clone(), number);
        report.added += 1;
    }
    if report.added > 0 {
        save(app_data, &map)?;
    }
    Ok(report)
}

/// Newest `.vcf` (modified in the last 90 days) in the usual download places.
pub fn find_vcf() -> Option<PathBuf> {
    let roots: Vec<PathBuf> = [dirs_next::download_dir(), dirs_next::desktop_dir(), dirs_next::document_dir()]
        .into_iter()
        .flatten()
        .collect();
    newest_vcf_in(&roots, std::time::Duration::from_secs(90 * 24 * 3600))
}

fn newest_vcf_in(roots: &[PathBuf], max_age: std::time::Duration) -> Option<PathBuf> {
    let now = std::time::SystemTime::now();
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("vcf")) {
                continue;
            }
            let Ok(modified) = e.metadata().and_then(|m| m.modified()) else { continue };
            if now.duration_since(modified).map_or(false, |age| age > max_age) {
                continue;
            }
            if best.as_ref().map_or(true, |(t, _)| modified > *t) {
                best = Some((modified, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// The outcome of looking a spoken name up.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    One { name: String, number: String },
    /// Several different people match: the user must say which.
    Ambiguous(Vec<String>),
    None,
}

fn norm_tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Look `candidates` (best first: learned alias, the spoken name, kinship synonyms) up in the book.
/// For each candidate: an exact name wins; otherwise a single contact whose name contains every token;
/// several such contacts = ambiguous. Pure.
pub fn lookup(book: &BTreeMap<String, String>, candidates: &[String]) -> Lookup {
    for cand in candidates {
        let want = norm_tokens(cand);
        if want.is_empty() {
            continue;
        }
        if let Some((name, number)) = book.iter().find(|(n, _)| norm_tokens(n) == want) {
            return Lookup::One { name: name.clone(), number: number.clone() };
        }
        let hits: Vec<(&String, &String)> = book
            .iter()
            .filter(|(n, _)| {
                let have = norm_tokens(n);
                want.iter().all(|w| have.iter().any(|h| h == w))
            })
            .collect();
        match hits.len() {
            0 => continue,
            1 => return Lookup::One { name: hits[0].0.clone(), number: hits[0].1.clone() },
            _ => return Lookup::Ambiguous(hits.iter().take(4).map(|(n, _)| (*n).clone()).collect()),
        }
    }
    Lookup::None
}

#[cfg(test)]
mod tests {
    use super::*;

    const VCF: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Mom\r\nN:;Mom;;;\r\nTEL;TYPE=CELL:+91 98765 43210\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nN:Kumar;Raj;;;\r\nitem1.TEL;type=pref:098765 11111\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nFN:Dentist\r\nTEL:022 2345 6789\r\nTEL:+91 99999 88888\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nFN:No Number\r\nEMAIL:a@b.com\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nFN:Folded\r\n Name\r\nTEL:+1 415 555 0100\r\nEND:VCARD\r\n";

    #[test]
    fn parses_names_fallback_n_groups_folding_and_skips_numberless_cards() {
        let cards = parse_vcf(VCF);
        let names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Mom", "Raj Kumar", "Dentist", "FoldedName"]);
        assert_eq!(cards[2].phones.len(), 2);
    }

    #[test]
    fn infers_the_users_country_code_from_plus_numbers() {
        let cards = parse_vcf(VCF);
        assert_eq!(infer_default_cc(&cards).as_deref(), Some("91"), "two +91 beat one +1");
        assert_eq!(infer_default_cc(&[]), None);
    }

    #[test]
    fn numbers_become_whatsapp_ready_or_are_refused() {
        assert_eq!(whatsapp_number("+91 98765 43210", Some("91")).as_deref(), Some("919876543210"));
        assert_eq!(whatsapp_number("098765 11111", Some("91")).as_deref(), Some("919876511111"));
        assert_eq!(whatsapp_number("98765 43210", Some("91")).as_deref(), Some("919876543210"));
        assert_eq!(whatsapp_number("0091 98765 43210", None).as_deref(), Some("919876543210"));
        assert_eq!(whatsapp_number("919876543210", Some("91")).as_deref(), Some("919876543210"), "already has the code");
        assert_eq!(whatsapp_number("98765 43210", None), None, "local number and no known country: refuse, never guess");
        assert_eq!(whatsapp_number("12345", Some("91")), None);
        assert_eq!(whatsapp_number("abc", Some("91")), None);
    }

    #[test]
    fn import_merges_without_overwriting_manual_entries() {
        let dir = std::env::temp_dir().join(format!("nexus_contacts_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("contacts.json"), r#"{"mom": "+910000000001"}"#).unwrap();
        let r = import_cards(&dir, &parse_vcf(VCF)).unwrap();
        assert_eq!(r.already_known, 1, "'Mom' is already there (any case)");
        assert_eq!(r.added, 3);
        let book = load(&dir);
        assert_eq!(book["mom"], "+910000000001", "the hand-written entry is untouched");
        assert_eq!(book["Raj Kumar"], "919876511111");
        assert_eq!(book["Dentist"], "919999988888", "the number with its own country code beats the local-format landline");
        let again = import_cards(&dir, &parse_vcf(VCF)).unwrap();
        assert_eq!(again.added, 0, "importing twice changes nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn book() -> BTreeMap<String, String> {
        [("Mom", "911"), ("Mom Work", "912"), ("Raj Kumar", "913"), ("Dr Raj", "914"), ("Asha", "915")]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn lookup_prefers_exact_then_a_unique_partial_and_flags_ambiguity() {
        let b = book();
        assert_eq!(lookup(&b, &["mom".into()]), Lookup::One { name: "Mom".into(), number: "911".into() }, "exact beats 'Mom Work'");
        assert_eq!(lookup(&b, &["asha".into()]), Lookup::One { name: "Asha".into(), number: "915".into() });
        assert_eq!(lookup(&b, &["kumar".into()]), Lookup::One { name: "Raj Kumar".into(), number: "913".into() });
        match lookup(&b, &["raj".into()]) {
            Lookup::Ambiguous(n) => assert_eq!(n.len(), 2),
            other => panic!("{other:?}"),
        }
        assert_eq!(lookup(&b, &["zorblax".into()]), Lookup::None);
    }

    #[test]
    fn spoken_mummy_finds_a_contact_saved_as_mom_through_the_candidates() {
        let b = book();
        let cands = crate::live::commands::wa_open::name_candidates("mummy", &[]);
        assert_eq!(lookup(&b, &cands), Lookup::One { name: "Mom".into(), number: "911".into() });
        // a learned alias goes first
        let cands = crate::live::commands::wa_open::name_candidates("mummy", &["Asha".to_string()]);
        assert_eq!(lookup(&b, &cands), Lookup::One { name: "Asha".into(), number: "915".into() });
    }

    #[test]
    fn finds_the_newest_vcf_and_ignores_old_and_other_files() {
        let d = std::env::temp_dir().join(format!("nexus_vcf_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("notes.txt"), "x").unwrap();
        std::fs::write(d.join("old.vcf"), "x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(d.join("contacts.VCF"), "x").unwrap();
        let got = newest_vcf_in(&[d.clone()], std::time::Duration::from_secs(3600)).unwrap();
        assert!(got.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("contacts.vcf"));
        assert!(newest_vcf_in(&[d.clone()], std::time::Duration::from_secs(0)).is_none(), "everything is older than 0 s");
        let _ = std::fs::remove_dir_all(&d);
    }
}
