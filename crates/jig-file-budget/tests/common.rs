#![allow(dead_code)]

use jig_file_budget::{
    ExactCurrentPathFactV1, ExactCurrentPathStateV1, PolicyDateV1, PolicyV1, parse_policy_v1,
};

pub fn date(year: u16, month: u8, day: u8) -> PolicyDateV1 {
    PolicyDateV1::new(year, month, day).unwrap()
}

pub fn policy(body: &str) -> PolicyV1 {
    parse_policy_v1(body.as_bytes(), date(2026, 8, 30)).unwrap()
}

pub fn line_policy(max_lines: u64) -> PolicyV1 {
    policy(&format!(
        r#"
version = 1

[[rules]]
id = "source"
include = ["**/*.rs", "*.rs"]
max_lines = {max_lines}
"#
    ))
}

pub fn two_metric_policy(extra: &str) -> PolicyV1 {
    policy(&format!(
        r#"
version = 1

[[rules]]
id = "source"
category = "source"
include = ["**/*"]
notice_lines = 4
warn_lines = 6
max_lines = 10
notice_bytes = 40
warn_bytes = 60
max_bytes = 100
{extra}
"#
    ))
}

pub fn regular_target(path: &str) -> ExactCurrentPathFactV1 {
    ExactCurrentPathFactV1 {
        path: path.to_owned(),
        state: ExactCurrentPathStateV1::Regular,
    }
}
