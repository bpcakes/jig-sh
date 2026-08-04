#[cfg(test)]
use std::io::BufRead;
use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result, anyhow};
use rustyline::{DefaultEditor, error::ReadlineError};

use super::output::{format_prompt_human_output, print_json, print_prompt_warnings};
use super::prompt::{PromptAddOpts, PromptCommand};
use crate::{context::RepoContext, runtime};

pub(super) fn run_prompt_command(command: PromptCommand, json_output: bool) -> Result<()> {
    let repo = RepoContext::load_optional()?;
    match command {
        PromptCommand::Get(opts) => {
            let output = runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::Get(crate::command::PromptRenderRequest {
                    name: opts.name,
                    vars: opts.vars,
                    raw: opts.raw,
                }),
            )?;
            if json_output {
                return print_json(&output);
            }
            let body = output
                .get(crate::command::PROMPT_BODY_KEY)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("prompt get output did not include body"))?;
            // Without structured output, `prompt get` is the raw prompt
            // primitive and stdout is exactly the rendered body.
            let mut stdout = io::stdout().lock();
            stdout.write_all(body.as_bytes())?;
            stdout.flush()?;
            Ok(())
        }
        PromptCommand::Copy(opts) => print_prompt_output(
            runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::Copy(crate::command::PromptRenderRequest {
                    name: opts.name,
                    vars: opts.vars,
                    raw: opts.raw,
                }),
            )?,
            json_output,
        ),
        PromptCommand::Add(opts) => print_prompt_output(
            runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::Add(prompt_add_request(opts)?),
            )?,
            json_output,
        ),
        PromptCommand::Edit(opts) => print_prompt_output(
            runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::Edit(crate::command::PromptEditRequest {
                    name: opts.name,
                    open_editor: !opts.no_editor,
                }),
            )?,
            json_output,
        ),
        PromptCommand::Remove(opts) => print_prompt_output(
            runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::Remove(crate::command::PromptNameRequest {
                    name: opts.name,
                }),
            )?,
            json_output,
        ),
        PromptCommand::List(opts) => print_prompt_output(
            runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::List(crate::command::PromptListRequest {
                    include_packs: !opts.no_packs,
                }),
            )?,
            json_output,
        ),
        PromptCommand::Search(opts) => print_prompt_output(
            runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::Search(crate::command::PromptSearchRequest {
                    query: opts.query,
                    include_body: opts.body,
                }),
            )?,
            json_output,
        ),
        PromptCommand::Export(opts) => {
            let print_archive = opts.output.is_none() && !json_output;
            let output = runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::Export(crate::command::PromptExportRequest {
                    output: opts.output,
                }),
            )?;
            if print_archive {
                // Without --output the archive itself is the requested artifact.
                print_json(&output["archive"])
            } else {
                print_prompt_output(output, json_output)
            }
        }
        PromptCommand::Import(opts) => print_prompt_output(
            runtime::dispatch_prompt(
                repo.as_ref(),
                crate::command::PromptCommand::Import(crate::command::PromptImportRequest {
                    file: opts.file,
                }),
            )?,
            json_output,
        ),
    }
}

fn prompt_add_request(opts: PromptAddOpts) -> Result<crate::command::PromptAddRequest> {
    if prompt_add_needs_interaction(&opts) {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            anyhow::bail!(
                "prompt add needs interactive input; pass NAME plus BODY or --file, or omit --no-editor to use $VISUAL or $EDITOR"
            );
        }
        interactive_prompt_add_request_terminal(opts)
    } else {
        let use_editor = prompt_add_uses_editor(&opts);
        Ok(crate::command::PromptAddRequest {
            name: opts.name.expect("checked by prompt_add_needs_interaction"),
            body: opts.body,
            file: opts.file,
            description: opts.description,
            tags: opts.tags,
            use_editor,
        })
    }
}

const fn prompt_add_needs_interaction(opts: &PromptAddOpts) -> bool {
    opts.name.is_none() || (opts.no_editor && opts.body.is_none() && opts.file.is_none())
}

const fn prompt_add_uses_editor(opts: &PromptAddOpts) -> bool {
    opts.name.is_some() && opts.body.is_none() && opts.file.is_none() && !opts.no_editor
}

fn interactive_prompt_add_request_terminal(
    opts: PromptAddOpts,
) -> Result<crate::command::PromptAddRequest> {
    let mut output = io::stderr();
    let editor = DefaultEditor::new().context("Failed to initialize prompt line editor")?;
    let mut prompt_io = TerminalPromptAddIo {
        editor,
        output: &mut output,
    };
    interactive_prompt_add_request_with_io(opts, &mut prompt_io)
}

trait PromptAddIo {
    fn write_line(&mut self, line: &str) -> Result<()>;
    fn optional_line(&mut self, prompt: &str) -> Result<Option<String>>;
    fn body_line(&mut self, prompt: &str) -> Result<Option<String>>;
}

fn interactive_prompt_add_request_with_io<I: PromptAddIo>(
    opts: PromptAddOpts,
    input: &mut I,
) -> Result<crate::command::PromptAddRequest> {
    input.write_line("Interactive prompt add")?;
    let name = match opts.name {
        Some(name) => name,
        None => prompt_required_line(input, "Prompt name: ", "prompt name")?,
    };
    let description = match opts.description {
        Some(description) => Some(description),
        None => {
            let value = input
                .optional_line("Description (optional): ")?
                .unwrap_or_default();
            if value.is_empty() { None } else { Some(value) }
        }
    };
    let tags = if opts.tags.is_empty() {
        let tags = input
            .optional_line("Tags (comma-separated, optional): ")?
            .unwrap_or_default();
        parse_interactive_tags(&tags)
    } else {
        opts.tags
    };
    let body = if opts.body.is_none() && opts.file.is_none() {
        Some(prompt_body(input)?)
    } else {
        opts.body
    };
    Ok(crate::command::PromptAddRequest {
        name,
        body,
        file: opts.file,
        description,
        tags,
        use_editor: false,
    })
}

#[cfg(test)]
pub(super) fn interactive_prompt_add_request<R: BufRead, W: Write>(
    opts: PromptAddOpts,
    input: R,
    output: &mut W,
) -> Result<crate::command::PromptAddRequest> {
    let mut prompt_io = BufferedPromptAddIo { input, output };
    interactive_prompt_add_request_with_io(opts, &mut prompt_io)
}

fn prompt_required_line<I: PromptAddIo>(
    input: &mut I,
    prompt: &str,
    label: &str,
) -> Result<String> {
    loop {
        let Some(line) = input.optional_line(prompt)? else {
            anyhow::bail!("interactive prompt add ended before {label} was complete");
        };
        if !line.is_empty() {
            return Ok(line);
        }
        input.write_line(&format!("{label} cannot be empty"))?;
    }
}

fn prompt_body<I: PromptAddIo>(input: &mut I) -> Result<String> {
    input.write_line("Prompt body. Finish with Ctrl-D or a line containing only a single dot.")?;
    let mut lines = Vec::new();
    loop {
        let Some(line) = input.body_line("> ")? else {
            if lines.is_empty() {
                anyhow::bail!("interactive prompt add ended before prompt body was complete")
            }
            break;
        };
        if line == "." {
            break;
        }
        lines.push(line);
    }
    let body = lines.join("\n");
    if body.trim().is_empty() {
        anyhow::bail!("prompt body cannot be empty");
    }
    Ok(body)
}

struct TerminalPromptAddIo<'a, W: Write> {
    editor: DefaultEditor,
    output: &'a mut W,
}

impl<W: Write> PromptAddIo for TerminalPromptAddIo<'_, W> {
    fn write_line(&mut self, line: &str) -> Result<()> {
        writeln!(self.output, "{line}")?;
        Ok(())
    }

    fn optional_line(&mut self, prompt: &str) -> Result<Option<String>> {
        match self.editor.readline(prompt) {
            Ok(line) => Ok(Some(line)),
            Err(ReadlineError::Interrupted) => anyhow::bail!("interactive prompt add interrupted"),
            Err(ReadlineError::Eof) => Ok(None),
            Err(error) => Err(error).context("Failed to read interactive prompt input"),
        }
    }

    fn body_line(&mut self, prompt: &str) -> Result<Option<String>> {
        match self.editor.readline(prompt) {
            Ok(line) => Ok(Some(line)),
            Err(ReadlineError::Interrupted) => anyhow::bail!("interactive prompt add interrupted"),
            Err(ReadlineError::Eof) => Ok(None),
            Err(error) => Err(error).context("Failed to read interactive prompt body"),
        }
    }
}

#[cfg(test)]
struct BufferedPromptAddIo<'a, R: BufRead, W: Write> {
    input: R,
    output: &'a mut W,
}

#[cfg(test)]
impl<R: BufRead, W: Write> PromptAddIo for BufferedPromptAddIo<'_, R, W> {
    fn write_line(&mut self, line: &str) -> Result<()> {
        writeln!(self.output, "{line}")?;
        Ok(())
    }

    fn optional_line(&mut self, prompt: &str) -> Result<Option<String>> {
        write!(self.output, "{prompt}")?;
        self.output.flush()?;
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(trim_line_ending(line)))
    }

    fn body_line(&mut self, prompt: &str) -> Result<Option<String>> {
        write!(self.output, "{prompt}")?;
        self.output.flush()?;
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(trim_line_ending(line)))
    }
}

#[cfg(test)]
fn trim_line_ending(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

fn parse_interactive_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

fn print_prompt_output(output: serde_json::Value, json_output: bool) -> Result<()> {
    if json_output {
        print_json(&output)
    } else {
        print_prompt_warnings(&output);
        print_human_summary(format_prompt_human_output(&output))
    }
}

fn print_human_summary(summary: String) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(summary.as_bytes())?;
    Ok(())
}

#[cfg(test)]
#[path = "prompt_run_tests.rs"]
mod tests;
