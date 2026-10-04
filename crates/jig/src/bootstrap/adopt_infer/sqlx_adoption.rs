//! Applies SQLx inference to adoption: warning provenance across the discovery
//! and selection passes, inferred answer defaults, and the effective review.

use std::path::Path;

use anyhow::{Result, bail};
use serde_json::json;

use super::metadata::Confidence;
use super::rust_sqlx::{MigrationChoice, infer_sqlx};
use super::scan::RepoScan;
use super::{AdoptInference, apply_sqlx_inference, fill_string};
use crate::bootstrap::AnswerOpts;
use crate::bootstrap::answers::{AnswerInputShape, EffectiveSqlx};

impl AdoptInference {
    /// Replaces SQLx inference from any earlier pass, including the warnings it
    /// raised, with inference over `scan`.
    pub(super) fn infer_and_apply_sqlx(&mut self, root: &Path, scan: &RepoScan) {
        self.retract_sqlx_warnings();
        let start = self.warnings.len();
        let sqlx = infer_sqlx(root, scan, &mut self.warnings);
        self.sqlx_warnings = self.warnings[start..].to_vec();
        apply_sqlx_inference(self, &sqlx);
    }

    /// Records that no component can own SQLx and withdraws earlier SQLx
    /// signals and warnings so they cannot attribute SQLx after selection.
    pub(super) fn clear_sqlx_inference(&mut self, reason: &str) {
        self.retract_sqlx_warnings();
        self.signals
            .retain(|signal| !self.sqlx_signals.contains(signal));
        self.sqlx_signals.clear();
        self.sqlx_enabled = Some(false);
        self.sqlx_migration_choice = MigrationChoice::default();
        self.rust_migration_dirs.clear();
        self.record_metadata(
            "sqlx_enabled",
            json!(false),
            vec![reason.into()],
            Confidence::High,
            Vec::new(),
        );
        self.rust_migration_dir = None;
        self.rust_sqlx_metadata_dir = None;
        self.sqlx_check_command = None;
    }

    /// Withdraws warnings about inferred SQLx defaults that will not be applied.
    /// Each tracked warning removes only its own, latest occurrence, so an
    /// identical warning raised by another inference pass survives.
    pub(in crate::bootstrap) fn retract_sqlx_warnings(&mut self) {
        for warning in std::mem::take(&mut self.sqlx_warnings) {
            if let Some(index) = self
                .warnings
                .iter()
                .rposition(|candidate| *candidate == warning)
            {
                self.warnings.remove(index);
            }
        }
    }

    pub(super) fn fill_sqlx_answers(
        &self,
        answers: &mut AnswerOpts,
        answer_shape: &AnswerInputShape,
    ) {
        let explicit_sqlx_enabled = answer_shape.explicit_sqlx_enabled(answers);
        let inference_decides = answer_shape.should_apply_inferred_sqlx_enabled(answers);
        if inference_decides {
            answers.sqlx_enabled = self.sqlx_enabled;
        }
        let detected = self.sqlx_enabled == Some(true);
        // Without an explicit answer, SQLx-shaped answers and schema dumps
        // imply SQLx, which resolution then enables.
        if !explicit_sqlx_enabled.unwrap_or(detected || !inference_decides) {
            return;
        }
        // Answers alone establish SQLx without evidence, so a migration
        // directory with a justified owner may supply its path; synthesized
        // defaults may not.
        let migration_dir = if detected {
            self.rust_migration_dir.as_deref()
        } else {
            self.sqlx_migration_choice.selected_dir()
        };
        if answers.migration_dir.is_none() && !answer_shape.contains_key("migration_dir") {
            fill_string(
                &mut answers.rust_migration_dir,
                migration_dir,
                answer_shape,
                "rust_migration_dir",
            );
        }
        if detected {
            fill_string(
                &mut answers.rust_sqlx_metadata_dir,
                self.rust_sqlx_metadata_dir.as_deref(),
                answer_shape,
                "rust_sqlx_metadata_dir",
            );
            fill_string(
                &mut answers.sqlx_check_command,
                self.sqlx_check_command.as_deref(),
                answer_shape,
                "sqlx_check_command",
            );
        }
    }

    /// Describes the SQLx answer that generated checks will use. Detected
    /// evidence stays in the detection report even when an answer disables it.
    pub(super) fn sqlx_review_item(&self, effective: &EffectiveSqlx) -> Option<String> {
        if !effective.enabled {
            return (self.sqlx_enabled == Some(true)).then(|| {
                "SQLx: disabled by explicit answer; detected SQLx evidence is not applied".into()
            });
        }
        Some(
            match (
                effective.migration_dir.as_deref(),
                self.unresolved_sqlx_migration(),
            ) {
                (Some(dir), _) => format!("SQLx: enabled with migrations at {dir}"),
                (None, Some(ambiguity)) => format!("SQLx: enabled, but Jig {ambiguity}"),
                (None, None) => {
                    "SQLx: enabled; confirm migration and metadata paths in .jig.toml".into()
                }
            },
        )
    }

    /// Stops adoption before rendering when SQLx is enabled but inference could
    /// not attribute a migration directory and no answer supplies one.
    pub(in crate::bootstrap) fn require_sqlx_migration_answer(
        &self,
        effective: &EffectiveSqlx,
    ) -> Result<()> {
        if effective.enabled
            && effective.migration_dir.is_none()
            && let Some(ambiguity) = self.unresolved_sqlx_migration()
        {
            bail!("SQLx is enabled, but Jig {ambiguity}");
        }
        Ok(())
    }

    fn unresolved_sqlx_migration(&self) -> Option<String> {
        // Preserved authored models never take inferred migration paths.
        if self.components.preserved {
            return None;
        }
        self.sqlx_migration_choice.ambiguity()
    }
}
