use std::path::PathBuf;

use clap::{ArgAction, Args, Subcommand};

const PROMPT_AFTER_HELP: &str = "\
Prompt bodies are rendered as MiniJinja templates unless --raw is passed.
Prompt names may be unqualified or explicitly namespaced with user:, repo:, or pack:.
Unqualified reads must resolve to exactly one prompt.
`jig prompt add NAME` with no BODY or --file opens $VISUAL or $EDITOR.
Use `jig prompt add --no-editor` to use the terminal prompt flow instead.
Finish the interactive body with Ctrl-D or a line containing only `.`.
Storage defaults to the user Jig config directory. JIG_PROMPT_HOME overrides
the storage root that contains prompts/user and prompt-packs.
`jig prompt get` prints only the rendered body unless --json requests a command envelope.
`jig prompt export` without --output prints the bare archive JSON artifact.
`jig --json prompt export` wraps that archive in a JSON command envelope.
`jig prompt export --output FILE` writes the archive and prints only a summary.
Re-adding an existing prompt preserves its description and tags unless new
values are provided.

Examples:
  jig prompt add
  jig prompt get comprehensive-review-loop
  jig prompt get repo:release-checklist --var base=main
  jig prompt get code-example --raw
  jig prompt add comprehensive-review-loop --file prompt.md --tag review
  jig prompt copy user:review-loop
  jig prompt export --output prompts.json
  jig prompt import prompts.json

Import replaces existing prompts and reports overwritten entries.
On Unix, EDITOR and VISUAL may include arguments such as `code -w`.";

#[derive(Debug, Subcommand)]
pub(crate) enum PromptCommand {
    /// Print a rendered prompt, using a command envelope with --json.
    #[command(alias = "cat")]
    Get(PromptGetOpts),
    /// Render a prompt body and copy it to the system clipboard.
    #[command(alias = "cp")]
    Copy(PromptCopyOpts),
    /// Add or replace a writable user: or repo: prompt.
    #[command(alias = "new")]
    Add(PromptAddOpts),
    /// Edit a writable user: or repo: prompt in $EDITOR.
    Edit(PromptEditOpts),
    /// Remove a writable user: or repo: prompt.
    #[command(alias = "rm")]
    Remove(PromptRemoveOpts),
    /// List prompt names and metadata without prompt bodies.
    #[command(alias = "ls")]
    List(PromptListOpts),
    /// Search prompt names, descriptions, tags, and optionally bodies.
    #[command(alias = "find")]
    Search(PromptSearchOpts),
    /// Export prompts and prompt packs as a versioned JSON archive.
    Export(PromptExportOpts),
    /// Import prompts and prompt packs from a versioned JSON archive.
    Import(PromptImportOpts),
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptGetOpts {
    /// Prompt name, optionally qualified with user:, repo:, or pack:.
    pub(crate) name: String,
    /// Template variable in key=value form. May be repeated.
    #[arg(long = "var", value_name = "KEY=VALUE", action = ArgAction::Append)]
    pub(crate) vars: Vec<String>,
    /// Print the stored prompt body without template rendering.
    #[arg(long, conflicts_with = "vars")]
    pub(crate) raw: bool,
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptCopyOpts {
    /// Prompt name, optionally qualified with user:, repo:, or pack:.
    pub(crate) name: String,
    /// Template variable in key=value form. May be repeated.
    #[arg(long = "var", value_name = "KEY=VALUE", action = ArgAction::Append)]
    pub(crate) vars: Vec<String>,
    /// Copy the stored prompt body without template rendering.
    #[arg(long, conflicts_with = "vars")]
    pub(crate) raw: bool,
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptAddOpts {
    /// Prompt name. Unqualified writes default to user:. Omit to enter interactive mode.
    pub(crate) name: Option<String>,
    /// Prompt body. Use shell quoting for multi-word prompts. Omit with --file absent to open an editor for named prompts.
    pub(crate) body: Option<String>,
    /// Read the prompt body from a file.
    #[arg(long, conflicts_with = "body")]
    pub(crate) file: Option<PathBuf>,
    /// Use the terminal prompt flow instead of launching $VISUAL or $EDITOR when BODY and --file are omitted.
    #[arg(long)]
    pub(crate) no_editor: bool,
    /// Metadata description shown by list/search.
    #[arg(long)]
    pub(crate) description: Option<String>,
    /// Metadata tag. May be repeated.
    #[arg(long = "tag", action = ArgAction::Append)]
    pub(crate) tags: Vec<String>,
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptEditOpts {
    /// Prompt name. Unqualified edits an existing unambiguous writable prompt, or user: when new.
    pub(crate) name: String,
    /// Print the resolved editable prompt path without launching $VISUAL or $EDITOR.
    #[arg(long)]
    pub(crate) no_editor: bool,
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptRemoveOpts {
    /// Prompt name. Unqualified deletes only when one writable prompt matches.
    pub(crate) name: String,
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptListOpts {
    /// Exclude prompt packs from the listing.
    #[arg(long)]
    pub(crate) no_packs: bool,
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptSearchOpts {
    /// Search query.
    pub(crate) query: String,
    /// Include prompt bodies in the search index.
    #[arg(long)]
    pub(crate) body: bool,
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptExportOpts {
    /// Write the archive to a file instead of stdout.
    #[arg(long)]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Args, Debug)]
#[command(after_help = PROMPT_AFTER_HELP)]
pub(crate) struct PromptImportOpts {
    /// Versioned JSON archive produced by `jig prompt export`.
    pub(crate) file: PathBuf,
}
