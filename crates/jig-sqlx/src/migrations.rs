use std::collections::BTreeMap;

/// Files that share a SQLx numeric version without forming one reversible pair.
#[derive(Debug, Eq, PartialEq)]
pub struct MigrationVersionConflict {
    pub version: i64,
    pub filenames: Vec<String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Direction {
    Simple,
    Up,
    Down,
}

/// Validate filenames from a single migration source, using SQLx's `i64` identity.
///
/// The caller selects direct files in that source. Unrecognized filenames are
/// ignored; filename/SQL validity remains the responsibility of SQLx itself.
/// A single file or one up/down pair is allowed for each numeric version.
pub fn migration_version_conflicts<'a>(
    filenames: impl IntoIterator<Item = &'a str>,
) -> Vec<MigrationVersionConflict> {
    let mut versions = BTreeMap::<i64, Vec<(&str, Direction)>>::new();
    for filename in filenames {
        let Some((prefix, description)) = filename.split_once('_') else {
            continue;
        };
        if !description.ends_with(".sql") {
            continue;
        }
        let Ok(version) = prefix.parse::<i64>() else {
            continue;
        };
        let direction = if description.ends_with(".up.sql") {
            Direction::Up
        } else if description.ends_with(".down.sql") {
            Direction::Down
        } else {
            Direction::Simple
        };
        versions
            .entry(version)
            .or_default()
            .push((filename, direction));
    }
    versions
        .into_iter()
        .filter(|(_, files)| {
            files.len() > 1
                && !(files.len() == 2
                    && files
                        .iter()
                        .any(|(_, direction)| *direction == Direction::Up)
                    && files
                        .iter()
                        .any(|(_, direction)| *direction == Direction::Down))
        })
        .map(|(version, files)| {
            let mut filenames = files
                .into_iter()
                .map(|(filename, _)| filename.to_owned())
                .collect::<Vec<_>>();
            filenames.sort();
            MigrationVersionConflict { version, filenames }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_identity_ignores_padding_and_reports_all_conflicting_files() {
        let conflicts = migration_version_conflicts([
            "2_second.sql",
            "01_first.sql",
            "1_other.sql",
            "+1_third.sql",
            "2_other.sql",
        ]);
        assert_eq!(
            conflicts,
            vec![
                MigrationVersionConflict {
                    version: 1,
                    filenames: vec![
                        "+1_third.sql".into(),
                        "01_first.sql".into(),
                        "1_other.sql".into()
                    ],
                },
                MigrationVersionConflict {
                    version: 2,
                    filenames: vec!["2_other.sql".into(), "2_second.sql".into()],
                },
            ]
        );
    }

    #[test]
    fn reversible_pairs_and_independent_sources_are_allowed() {
        assert!(migration_version_conflicts(["1_pair.up.sql", "01_pair.down.sql"]).is_empty());
        assert!(migration_version_conflicts(["1_first.sql"]).is_empty());
        assert!(migration_version_conflicts(["1_independent.sql"]).is_empty());
    }

    #[test]
    fn duplicate_directions_and_simple_reversible_collisions_are_rejected() {
        for files in [
            vec!["1_first.up.sql", "1_second.up.sql"],
            vec!["1_first.down.sql", "1_second.down.sql"],
            vec!["1_simple.sql", "1_pair.up.sql"],
            vec!["1_simple.sql", "1_pair.down.sql"],
            vec!["1_first.up.sql", "1_second.up.sql", "1_pair.down.sql"],
            vec!["1_simple.sql", "1_pair.up.sql", "1_pair.down.sql"],
        ] {
            let conflicts = migration_version_conflicts(files.iter().copied());
            assert_eq!(conflicts.len(), 1, "{files:?}");
            assert_eq!(conflicts[0].filenames.len(), files.len(), "{files:?}");
        }
    }

    #[test]
    fn unrecognized_names_and_i64_overflow_do_not_create_false_collisions() {
        assert!(
            migration_version_conflicts([
                ".gitkeep",
                "1.sql",
                "1_notes.txt",
                "notes_first.sql",
                "notes_second.sql",
                "9223372036854775808_first.sql",
                "9223372036854775808_second.sql",
            ])
            .is_empty()
        );
        let conflicts = migration_version_conflicts([
            "9223372036854775807_first.sql",
            "9223372036854775807_second.sql",
        ]);
        assert_eq!(conflicts[0].version, i64::MAX);
    }
}
