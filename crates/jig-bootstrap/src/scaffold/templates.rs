use anyhow::{Context, Result, bail};
use minijinja::{Environment, UndefinedBehavior, syntax::SyntaxConfig};
use serde_json::Value;

use super::embedded_templates::{EMBEDDED_SCAFFOLD_TEMPLATE_FILES, EmbeddedScaffoldTemplateFile};

const TEMPLATE_SUFFIX: &str = ".jinja";

#[derive(Clone, Copy, Debug)]
pub(super) struct ScaffoldTemplateFile {
    pub(super) template: &'static str,
    pub(super) output: &'static str,
}

pub(super) fn render_scaffold_template(template: &str, context: &Value) -> Result<String> {
    render_scaffold_template_from_files(EMBEDDED_SCAFFOLD_TEMPLATE_FILES, template, context)
}

pub(super) fn render_scaffold_template_from_files(
    files: &[EmbeddedScaffoldTemplateFile],
    template: &str,
    context: &Value,
) -> Result<String> {
    let source = files
        .iter()
        .find(|file| file.relative_path == template)
        .map(|file| file.contents)
        .ok_or_else(|| anyhow::anyhow!("Scaffold template {template} was not embedded"))?;
    let environment = scaffold_environment()?;
    environment
        .render_str(source, context)
        .with_context(|| format!("Failed to render scaffold template {template}"))
}

pub(super) fn ensure_scaffold_template_paths(files: &[ScaffoldTemplateFile]) -> Result<()> {
    for file in files {
        if !file.template.ends_with(TEMPLATE_SUFFIX) {
            bail!(
                "Scaffold template {} must end with {TEMPLATE_SUFFIX}",
                file.template
            );
        }
        if !EMBEDDED_SCAFFOLD_TEMPLATE_FILES
            .iter()
            .any(|template| template.relative_path == file.template)
        {
            bail!("Scaffold template {} was not embedded", file.template);
        }
    }
    Ok(())
}

fn scaffold_environment() -> Result<Environment<'static>> {
    let mut environment = Environment::new();
    environment.set_syntax(
        SyntaxConfig::builder()
            .block_delimiters("[%", "%]")
            .variable_delimiters("<<[", "]>>")
            .comment_delimiters("<#", "#>")
            .build()?,
    );
    environment.set_trim_blocks(true);
    environment.set_lstrip_blocks(true);
    environment.set_keep_trailing_newline(true);
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    Ok(environment)
}
