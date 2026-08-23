#![cfg(any(target_os = "linux", target_os = "macos"))]

mod support;

include!("dev_sigint_parts/part_01.rs");
include!("dev_sigint_parts/part_02.rs");
include!("dev_sigint_parts/part_03.rs");
