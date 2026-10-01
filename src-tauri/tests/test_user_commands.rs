// Test the new PR/branch query commands
// Run: cargo test --test test_user_commands -- --nocapture

use nexus_lib::intent_parser::{parse_deterministic, ParsedIntent, ParseResult};

#[allow(dead_code)]
fn get_result(input: &str) -> String {
    match parse_deterministic(input) {
        Some(r) => format!("{:?}", r.intent),
        None => "None".to_string(),
    }
}

// ─── User's 3 exact commands ───

#[test]
fn test_analyse_pr_in_zync_no_number() {
    let result = parse_deterministic("analyse the pr in zync");
    assert!(matches!(
        result,
        Some(ParseResult {
            intent: ParsedIntent::AnalyseLatestPr { ref repo, author: None, .. },
            ..
        }) if repo == "zync"
    ), "Expected AnalyseLatestPr {{repo: zync, author: None}}, got {:?}", result.as_ref().map(|r| format!("{:?}", r.intent)));
    println!("✅ analyse the pr in zync → {:?}", result.as_ref().map(|r| format!("{:?}", r.intent)));
}

#[test]
fn test_analyse_pr_of_prem_in_servx() {
    let result = parse_deterministic("analyse the pr of prem in servx");
    assert!(matches!(
        result,
        Some(ParseResult {
            intent: ParsedIntent::AnalyseLatestPr { ref repo, author: Some(ref a), .. },
            ..
        }) if repo == "servx" && a == "prem"
    ), "Expected AnalyseLatestPr {{repo: servx, author: prem}}, got {:?}", result.as_ref().map(|r| format!("{:?}", r.intent)));
    println!("✅ analyse the pr of prem in servx → {:?}", result.as_ref().map(|r| format!("{:?}", r.intent)));
}

#[test]
fn test_check_latest_branch_servx_by_eesha() {
    let result = parse_deterministic("check the latest branch of servx created by eesha");
    assert!(matches!(
        result,
        Some(ParseResult {
            intent: ParsedIntent::CheckBranch { ref repo, author: Some(ref a), .. },
            ..
        }) if repo == "servx" && a == "eesha"
    ), "Expected CheckBranch {{repo: servx, author: eesha}}, got {:?}", result.as_ref().map(|r| format!("{:?}", r.intent)));
    println!("✅ check the latest branch of servx created by eesha → {:?}", result.as_ref().map(|r| format!("{:?}", r.intent)));
}

// ─── Latest PR variations ───

#[test]
fn test_latest_pr_variations() {
    let cases = vec![
        // (input, expected_repo, expected_author)
        ("analyse the pr in zync", "zync", None),
        ("analyse the latest pr in zync", "zync", None),
        ("analyse latest pr zync", "zync", None),
        ("analyse pr in zync", "zync", None),
        ("analyse newest pr in zync", "zync", None),
        ("analyse the pull request in zync", "zync", None),
        ("analyse latest pull request in zync", "zync", None),
        ("analyse the pr of zync", "zync", None),
        ("analyse recent pr in zync", "zync", None),
        ("analyse the pr by prem in servx", "servx", Some("prem")),
        ("analyse pr by prem in servx", "servx", Some("prem")),
        ("analyse pr of prem in servx", "servx", Some("prem")),
        ("analyse latest pr by prem in servx", "servx", Some("prem")),
        ("analyse the latest pr by prem in servx", "servx", Some("prem")),
        ("analyse the pull request by prem in servx", "servx", Some("prem")),
        ("analyse the pull request of prem in servx", "servx", Some("prem")),
        ("analyse pr from prem in servx", "servx", Some("prem")),
        ("analyse latest pr from prem in servx", "servx", Some("prem")),
    ];

    let mut passed = 0;
    let mut failed = 0;

    for (input, expected_repo, expected_author) in cases {
        let result = parse_deterministic(input);
        let ok = match &result {
            Some(ParseResult {
                intent: ParsedIntent::AnalyseLatestPr { repo, author, .. },
                ..
            }) => {
                repo == expected_repo && author.as_deref() == expected_author
            }
            _ => false,
        };
        if ok {
            println!("✅ {:50} → repo={}, author={:?}", input, expected_repo, expected_author);
            passed += 1;
        } else {
            println!("❌ {:50} → got {:?} (expected repo={}, author={:?})", input, result.map(|r| format!("{:?}", r.intent)), expected_repo, expected_author);
            failed += 1;
        }
    }

    println!("\nLatest PR: {} passed, {} failed", passed, failed);
    assert!(failed == 0, "{} latest PR variations failed", failed);
}

// ─── Branch by author variations ───

#[test]
fn test_branch_by_author_variations() {
    let cases = vec![
        // (input, expected_repo, expected_author)
        ("check the latest branch of servx created by eesha", "servx", Some("eesha")),
        ("check the latest branch in servx created by eesha", "servx", Some("eesha")),
        ("check latest branch of servx by eesha", "servx", Some("eesha")),
        ("check branch of servx by eesha", "servx", Some("eesha")),
        ("check the branch of servx created by eesha", "servx", Some("eesha")),
        ("check the latest branch of servx by eesha", "servx", Some("eesha")),
        ("check the latest branch in servx by eesha", "servx", Some("eesha")),
        ("check the latest branch by eesha in servx", "servx", Some("eesha")),
        ("show the latest branch of servx created by eesha", "servx", Some("eesha")),
        ("show latest branch of servx by eesha", "servx", Some("eesha")),
        ("show the latest branch of servx by eesha", "servx", Some("eesha")),
        ("what is the latest branch of servx created by eesha", "servx", Some("eesha")),
        ("what is the latest branch by eesha in servx", "servx", Some("eesha")),
        ("check the newest branch of servx created by eesha", "servx", Some("eesha")),
        ("check the recent branch of servx created by eesha", "servx", Some("eesha")),
    ];

    let mut passed = 0;
    let mut failed = 0;

    for (input, expected_repo, expected_author) in cases {
        let result = parse_deterministic(input);
        let ok = match &result {
            Some(ParseResult {
                intent: ParsedIntent::CheckBranch { repo, author, .. },
                ..
            }) => {
                repo == expected_repo && author.as_deref() == expected_author
            }
            _ => false,
        };
        if ok {
            println!("✅ {:60} → repo={}, author={:?}", input, expected_repo, expected_author);
            passed += 1;
        } else {
            println!("❌ {:60} → got {:?} (expected repo={}, author={:?})", input, result.map(|r| format!("{:?}", r.intent)), expected_repo, expected_author);
            failed += 1;
        }
    }

    println!("\nBranch: {} passed, {} failed", passed, failed);
    assert!(failed == 0, "{} branch variations failed", failed);
}

// ─── Ensure existing PR commands still work ───

#[test]
fn test_existing_pr_commands_still_work() {
    let cases = vec![
        "analyse PR 23 servx",
        "analyse PR 23 in servx",
        "analyse pr 23 servx",
        "analyse pull request 23 servx",
        "analyse the pr 254 in zync",
        "analyse PR 5 zync-meet/zync",
    ];

    for input in cases {
        let result = parse_deterministic(input);
        assert!(
            matches!(
                result,
                Some(ParseResult {
                    intent: ParsedIntent::AnalysePr { .. },
                    ..
                })
            ),
            "Expected AnalysePr for '{}', got {:?}",
            input,
            result.map(|r| format!("{:?}", r.intent))
        );
        println!("✅ {} → AnalysePr (still works)", input);
    }
}

// ─── Ensure existing repo commands still work ───

#[test]
fn test_existing_repo_commands_still_work() {
    let cases = vec![
        "analyse servx repo",
        "analyse servx",
        "analyse zync",
        "analyse zync-meet/zync",
        "analyse the repo servx",
        "analyse repo servx",
    ];

    for input in cases {
        let result = parse_deterministic(input);
        assert!(
            matches!(
                result,
                Some(ParseResult {
                    intent: ParsedIntent::AnalyseRepo { .. },
                    ..
                })
            ),
            "Expected AnalyseRepo for '{}', got {:?}",
            input,
            result.map(|r| format!("{:?}", r.intent))
        );
        println!("✅ {} → AnalyseRepo (still works)", input);
    }
}
