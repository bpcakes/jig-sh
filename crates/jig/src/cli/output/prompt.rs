use serde_json::Value;

pub(in crate::cli) fn format_prompt_human_output(output: &Value) -> String {
    let command = output["command"].as_str().unwrap_or("prompt");
    match command {
        "prompt list" | "prompt search" => {
            let mut lines = Vec::new();
            for prompt in output["prompts"].as_array().into_iter().flatten() {
                lines.push(format_prompt_summary_line(prompt));
            }
            if lines.is_empty() {
                lines.push("no prompts".to_string());
            }
            lines.push(String::new());
            lines.join("\n")
        }
        "prompt copy" => format!(
            "{command}: {}\n",
            output["qualified_name"]
                .as_str()
                .or_else(|| output["name"].as_str())
                .unwrap_or("")
        ),
        "prompt edit" if output["editor"].as_bool() == Some(false) => format!(
            "{command}: {}\npath: {}\n",
            output["name"].as_str().unwrap_or(""),
            output["path"].as_str().unwrap_or("")
        ),
        "prompt add" | "prompt edit" | "prompt remove" => {
            format!("{command}: {}\n", output["name"].as_str().unwrap_or(""))
        }
        "prompt import" => {
            let count = output["imported"].as_array().map(Vec::len).unwrap_or(0);
            let overwritten = output["imported"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|entry| entry["overwritten"].as_bool() == Some(true))
                .count();
            format!("prompt import: {count} prompts imported, {overwritten} overwritten\n")
        }
        "prompt export" => {
            let count = output["prompt_count"].as_u64().unwrap_or(0);
            if let Some(path) = output["output"].as_str() {
                format!("prompt export: {count} prompts written to {path}\n")
            } else {
                format!("prompt export: {count} prompts\n")
            }
        }
        _ => format!("{command}: ok\n"),
    }
}

fn format_prompt_summary_line(prompt: &Value) -> String {
    let name = prompt["qualified_name"].as_str().unwrap_or("");
    let description = prompt["description"].as_str().unwrap_or("");
    let tags = prompt["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    match (description.is_empty(), tags.is_empty()) {
        (true, true) => name.to_string(),
        (false, true) => format!("{name}\t{description}"),
        (true, false) => format!("{name}\t[{}]", tags.join(",")),
        (false, false) => format!("{name}\t{description}\t[{}]", tags.join(",")),
    }
}

pub(in crate::cli) fn print_prompt_warnings(output: &Value) {
    if let Some(warnings) = output["warnings"].as_array() {
        for warning in warnings.iter().filter_map(Value::as_str) {
            eprintln!("warning: {warning}");
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::format_prompt_human_output;

    #[test]
    fn edit_target_summary_includes_name_and_path() {
        let target = json!({
            "command": "prompt edit",
            "editor": false,
            "name": "new-prompt",
            "path": "store/prompts/user/new-prompt.md",
        });

        let human = format_prompt_human_output(&target);

        assert!(human.contains("prompt edit: new-prompt"));
        assert!(human.contains("store/prompts/user/new-prompt.md"));
    }
}
