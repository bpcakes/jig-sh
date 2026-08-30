use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{ExclusionV1, RuleV1, WaiverV1};

#[derive(Serialize)]
struct CanonicalPolicyV1<'a> {
    version: u32,
    rules: Vec<CanonicalRuleV1<'a>>,
    exclusions: Vec<&'a ExclusionV1>,
    waivers: Vec<&'a WaiverV1>,
}

#[derive(Serialize)]
struct CanonicalRuleV1<'a> {
    id: &'a str,
    category: &'a Option<String>,
    include: Vec<&'a str>,
    exclude: Vec<&'a str>,
    notice_lines: Option<u64>,
    warn_lines: Option<u64>,
    max_lines: Option<u64>,
    notice_bytes: Option<u64>,
    warn_bytes: Option<u64>,
    max_bytes: Option<u64>,
}

pub(super) fn canonical_semantic_input(
    rules: &[RuleV1],
    exclusions: &[ExclusionV1],
    waivers: &[WaiverV1],
) -> Vec<u8> {
    let mut canonical_rules = rules
        .iter()
        .map(|rule| {
            let mut include = rule.include.iter().map(String::as_str).collect::<Vec<_>>();
            include.sort_unstable();
            include.dedup();
            let mut exclude = rule.exclude.iter().map(String::as_str).collect::<Vec<_>>();
            exclude.sort_unstable();
            exclude.dedup();
            CanonicalRuleV1 {
                id: &rule.id,
                category: &rule.category,
                include,
                exclude,
                notice_lines: rule.notice_lines,
                warn_lines: rule.warn_lines,
                max_lines: rule.max_lines,
                notice_bytes: rule.notice_bytes,
                warn_bytes: rule.warn_bytes,
                max_bytes: rule.max_bytes,
            }
        })
        .collect::<Vec<_>>();
    canonical_rules.sort_unstable_by_key(|rule| rule.id);
    let mut canonical_exclusions = exclusions.iter().collect::<Vec<_>>();
    canonical_exclusions.sort_unstable_by(|left, right| {
        (&left.pattern, left.kind, &left.reason).cmp(&(&right.pattern, right.kind, &right.reason))
    });
    let mut canonical_waivers = waivers.iter().collect::<Vec<_>>();
    canonical_waivers.sort_unstable_by_key(|waiver| waiver.id.as_str());
    serde_json::to_vec(&CanonicalPolicyV1 {
        version: 1,
        rules: canonical_rules,
        exclusions: canonical_exclusions,
        waivers: canonical_waivers,
    })
    .expect("the version 1 semantic policy model is serializable")
}

pub(super) fn is_valid_date(year: u16, month: u8, day: u8) -> bool {
    if year == 0 || year > 9999 || !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= days
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}
