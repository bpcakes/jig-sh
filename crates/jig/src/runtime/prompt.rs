use anyhow::Result;
use serde_json::{Value, json};

use crate::command::{
    PROMPT_BODY_KEY, PromptAddRequest, PromptCommand, PromptEditRequest, PromptExportRequest,
    PromptImportRequest, PromptListRequest, PromptNameRequest, PromptRenderRequest,
    PromptSearchRequest,
};
use crate::context::RepoContext;
use crate::prompt_registry::{PromptRegistry, write_bytes_atomic};

pub(super) fn dispatch(ctx: Option<&RepoContext>, command: PromptCommand) -> Result<Value> {
    let registry = PromptRegistry::from_env(ctx.map(RepoContext::root))?;
    match command {
        PromptCommand::Get(request) => get_prompt(&registry, request),
        PromptCommand::Copy(request) => registry.copy_prompt(request),
        PromptCommand::Add(request) => add_prompt(&registry, request),
        PromptCommand::Edit(request) => edit_prompt(&registry, request),
        PromptCommand::Remove(request) => remove_prompt(&registry, request),
        PromptCommand::List(request) => list_prompts(&registry, request),
        PromptCommand::Search(request) => search_prompts(&registry, request),
        PromptCommand::Export(request) => export_prompts(&registry, request),
        PromptCommand::Import(request) => import_prompts(&registry, request),
    }
}

fn get_prompt(registry: &PromptRegistry, request: PromptRenderRequest) -> Result<Value> {
    let raw = request.raw;
    let body = registry.render_prompt(request)?;
    // Human CLI output selects `body`; JSON mode emits the complete envelope,
    // keeping prompt get shaped like other prompt runtime operations.
    let mut output = json!({
        "ok": true,
        "command": "prompt get",
        "raw": raw,
    });
    output[PROMPT_BODY_KEY] = Value::String(body);
    Ok(output)
}

fn add_prompt(registry: &PromptRegistry, request: PromptAddRequest) -> Result<Value> {
    let use_editor = request.use_editor;
    let request = crate::prompt_registry::PromptAddRequest {
        name: request.name,
        body: request.body,
        file: request.file,
        description: request.description,
        tags: request.tags,
    };
    if use_editor {
        registry.add_prompt_with_editor(request)
    } else {
        registry.add_prompt(request)
    }
}

fn edit_prompt(registry: &PromptRegistry, request: PromptEditRequest) -> Result<Value> {
    if request.open_editor {
        registry.edit_prompt(&request.name)
    } else {
        registry.prompt_edit_target(&request.name)
    }
}

fn remove_prompt(registry: &PromptRegistry, request: PromptNameRequest) -> Result<Value> {
    registry.remove_prompt(&request.name)
}

fn list_prompts(registry: &PromptRegistry, request: PromptListRequest) -> Result<Value> {
    registry.list_prompts(request.include_packs)
}

fn search_prompts(registry: &PromptRegistry, request: PromptSearchRequest) -> Result<Value> {
    registry.search_prompts(&request.query, request.include_body)
}

fn export_prompts(registry: &PromptRegistry, request: PromptExportRequest) -> Result<Value> {
    let archive = registry.export_prompts()?;
    let prompt_count = archive["prompts"].as_array().map(Vec::len).unwrap_or(0);
    if let Some(output) = request.output {
        let text = serde_json::to_string_pretty(&archive)?;
        write_bytes_atomic(&output, format!("{text}\n").as_bytes())?;
        Ok(json!({
            "ok": true,
            "command": "prompt export",
            "output": output,
            "prompt_count": prompt_count,
        }))
    } else {
        Ok(json!({
            "ok": true,
            "command": "prompt export",
            "prompt_count": prompt_count,
            "archive": archive,
        }))
    }
}

fn import_prompts(registry: &PromptRegistry, request: PromptImportRequest) -> Result<Value> {
    registry.import_prompts(&request.file)
}
