fn pr_worker_output_schema(max_review_thread_replies: usize) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "review_thread_replies"],
        "properties": {
            "summary": {
                "type": "string",
                "description": "Concise summary of the repair attempt."
            },
            "review_thread_replies": {
                "type": "array",
                "description": "GitHub review thread reply intents for Jig to post outside the sandbox.",
                "maxItems": max_review_thread_replies,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["thread_id"],
                    "properties": {
                        "thread_id": {
                            "type": "string",
                            "description": "GitHub pull request review thread node ID, such as PRRT_..."
                        },
                        "body": {
                            "type": "string",
                            "description": "Reply body to post. Leave empty only for resolve-only actions."
                        },
                        "resolve": {
                            "type": "boolean",
                            "description": "Whether Jig should resolve the review thread after posting any reply."
                        }
                    }
                }
            }
        }
    })
}

fn parse_pr_worker_output(stdout: &[u8]) -> Result<Value> {
    if stdout.is_empty() {
        bail!("PR manager worker did not write structured output");
    }
    let value = serde_json::from_slice::<Value>(stdout)
        .context("Failed to parse PR manager worker structured output")?;
    let replies = value
        .get("review_thread_replies")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("PR manager worker output did not include review_thread_replies"))?;
    for reply in replies {
        let thread_id = reply
            .get("thread_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if thread_id.is_empty() {
            bail!("PR manager worker output included a reply without thread_id");
        }
        let body = reply
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let resolve = reply
            .get("resolve")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if body.is_empty() && !resolve {
            bail!("PR manager worker output for thread {thread_id} has no body or resolve action");
        }
    }
    Ok(value)
}
