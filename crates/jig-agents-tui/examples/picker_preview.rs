//! Opens the home picker with example homes and simulated inspection, for
//! working on its presentation without real accounts:
//!
//! ```sh
//! cargo run -p jig-agents-tui --example picker_preview
//! ```
//!
//! Nothing is launched; the selection is printed after the picker closes.

use std::{
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jig_agents_tui::{ConfigurationHome, Home, HomeUpdate, InspectionSource};
use serde_json::{Value, json};

const HOMES: [(&str, &str, f64, f64); 5] = [
    ("example", "person@example.com", 15.0, 45.0),
    ("example-work", "work@example.com", 18.0, 17.0),
    ("example-team", "team@example.com", 2.0, 96.0),
    ("example-lab", "lab@example.com", 64.0, 40.0),
    ("example-broken", "", 0.0, 0.0),
];

struct SimulatedInspection;

impl InspectionSource for SimulatedInspection {
    fn inspect(
        &self,
        emit: &mut dyn FnMut(HomeUpdate) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs();
        for (index, (_, email, five_hour, weekly)) in HOMES.into_iter().enumerate() {
            for _ in 0..6 {
                if cancelled() {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(100));
            }
            let details = if email.is_empty() {
                json!({"account": null, "status": "unknown", "rate_limits": [],
                       "inspection_error": "example inspection failure"})
            } else {
                usage(now, index as u64, email, five_hour, weekly)
            };
            emit(HomeUpdate { index, details })?;
        }
        Ok(())
    }
}

fn usage(now: u64, index: u64, email: &str, five_hour: f64, weekly: f64) -> Value {
    json!({
        "account": {"type": "Example", "email": email, "plan_type": "max"},
        "status": "authenticated",
        "rate_limits": [
            {"id": "example", "name": "Example",
             "primary": {"used_percent": five_hour, "duration_minutes": 300,
                         "resets_at": now + 7_200 + index * 1_800},
             "secondary": {"used_percent": weekly, "duration_minutes": 10_080,
                           "resets_at": now + 100_000 + index * 40_000}},
            {"id": "example-model", "name": "Model",
             "primary": {"used_percent": weekly / 3.0, "duration_minutes": 10_080,
                         "resets_at": now + 100_000}}
        ]
    })
}

fn main() -> anyhow::Result<()> {
    let homes = HOMES
        .iter()
        .enumerate()
        .map(|(index, (name, ..))| ConfigurationHome {
            home: Home {
                path: format!("/tmp/ExampleHome/.{name}").into(),
                name: (*name).to_owned(),
                current: index == 0,
            },
            details: vec![("CONFIG_DIR".to_owned(), format!("/tmp/ExampleHome/.{name}"))],
        })
        .collect();
    let selected = jig_agents_tui::select_provider_with_cancellation(
        "Example Home Picker",
        "picker_preview",
        homes,
        Vec::new(),
        Some(SimulatedInspection),
        Some("example"),
        || false,
    )?;
    match selected {
        Some(index) => println!("Selected {}", HOMES[index].0),
        None => println!("Cancelled"),
    }
    Ok(())
}
