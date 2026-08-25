use std::path::PathBuf;

use anyhow::Result;
use serde_json::json;

use super::super::answers::{web_install_command, web_run_command};
use super::super::{
    GENERATED_NODE_TYPES_VERSION, GENERATED_NODE_VERSION, generated_package_manager_spec,
    generated_package_manager_version,
};
use super::embedded_templates::EMBEDDED_SCAFFOLD_TEMPLATE_FILES;
use super::names::{bounded_postgres_identifier, normalize_package_name, validate_scaffold_name};
use super::templates::{
    ScaffoldTemplateFile, ensure_scaffold_template_paths, render_scaffold_template,
};
use super::write::{ScaffoldFile, scaffold_file};
use super::{FrontendApp, ScaffoldDb, ScaffoldFrontend, ScaffoldFrontendKind, ScaffoldPreset};
include!("frontend_parts/part_01.rs");
include!("frontend_parts/part_02.rs");
include!("frontend_parts/part_03.rs");
