use anyhow::Result;
use serde_json::{Value, json};

use super::{Home, Homes};

mod credentials;
mod http;
#[cfg(target_os = "macos")]
mod keychain;
mod normalize;
#[cfg(test)]
mod tests;

pub(crate) struct Inspection {
    homes: Vec<Home>,
    allow_keychain_prompt: bool,
}

impl Inspection {
    pub(crate) fn new(homes: Vec<Home>, allow_keychain_prompt: bool) -> Self {
        Self {
            homes,
            allow_keychain_prompt,
        }
    }

    pub(crate) fn inspect(
        &self,
        emit: &mut dyn FnMut(usize, Value) -> Result<(), String>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Could not initialize Claude usage inspection")?;
        let client = http::client()?;
        for (index, home) in self.homes.iter().enumerate() {
            if cancelled() {
                break;
            }
            let details = match credentials::load(home, self.allow_keychain_prompt, cancelled) {
                Ok(credential) => {
                    let usage = if let Some(error) =
                        credential.usage_error(chrono::Utc::now().timestamp_millis())
                    {
                        Err(error)
                    } else {
                        runtime
                            .block_on(http::fetch(
                                &client,
                                &credential.access_token,
                                http::USAGE_URL,
                                cancelled,
                            ))
                            .and_then(|value| normalize::limits(&value))
                    };
                    inspected(&credential, usage)
                }
                Err(error) => {
                    json!({"account":null,"status":"unknown","rate_limits":[],"inspection_error":error,"usage_error":null})
                }
            };
            if cancelled() {
                break;
            }
            emit(index, details)?;
        }
        Ok(())
    }
}

fn inspected(credential: &credentials::Credential, usage: Result<Vec<Value>, String>) -> Value {
    let (limits, error) = match usage {
        Ok(limits) => (limits, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    json!({
        "account":{"type":"Claude", "email":null, "plan_type":credential.subscription_type},
        "status":"authenticated", "rate_limits":limits,
        "usage_included":true, "inspection_error":null, "usage_error":error,
    })
}

pub(crate) fn report(homes: Homes, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Value> {
    let mut report = homes.report();
    report["usage_included"] = json!(true);
    let inspection = Inspection::new(homes.selections(), false);
    inspection
        .inspect(
            &mut |index, details| {
                if !details["inspection_error"].is_null() || !details["usage_error"].is_null() {
                    report["outcome"] = json!("partial");
                }
                if let (Some(home), Some(details)) =
                    (report["homes"][index].as_object_mut(), details.as_object())
                {
                    home.extend(details.clone());
                }
                Ok(())
            },
            cancelled,
        )
        .map_err(anyhow::Error::msg)?;
    if cancelled() {
        anyhow::bail!("Claude usage inspection was cancelled");
    }
    Ok(report)
}
