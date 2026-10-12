use std::time::Duration;

use reqwest::{Client, StatusCode, header};
use serde_json::Value;

pub(super) const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const RESPONSE_LIMIT: usize = 64 * 1024;

pub(super) fn client() -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .user_agent(concat!("jig/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "Could not initialize Claude usage HTTP client".into())
}

pub(super) async fn fetch(
    client: &Client,
    token: &str,
    url: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Value, String> {
    let request = async {
        let mut authorization = header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "Claude OAuth token is not a valid HTTP credential")?;
        authorization.set_sensitive(true);
        let mut response = client
            .get(url)
            .header(header::AUTHORIZATION, authorization)
            .header("anthropic-beta", "oauth-2025-04-20")
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| "Claude usage request failed or timed out")?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => {
                return Err("Claude login expired; sign in again with Claude".into());
            }
            StatusCode::FORBIDDEN => {
                return Err("Claude login cannot read subscription usage".into());
            }
            StatusCode::TOO_MANY_REQUESTS => {
                return Err("Claude usage is rate limited; try again later".into());
            }
            status => {
                return Err(format!(
                    "Claude usage request returned HTTP {}",
                    status.as_u16()
                ));
            }
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Could not read Claude usage response")?
        {
            if chunk.len() > RESPONSE_LIMIT.saturating_sub(body.len()) {
                return Err("Claude usage response exceeded the size limit".into());
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| "Claude usage response was not valid JSON".into())
    };
    tokio::pin!(request);
    loop {
        if cancelled() {
            return Err("Claude usage inspection was cancelled".into());
        }
        tokio::select! {
            result = &mut request => return result,
            () = tokio::time::sleep(Duration::from_millis(25)) => {},
        }
    }
}
