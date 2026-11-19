use security_framework::base::{Error, Result as SearchOutcome};
use security_framework::os::macos::keychain::SecKeychain;

use super::*;

enum Expected {
    Stored,
    Missing,
    ToolRead,
    MissingAfterProbe,
    InconclusiveAfterProbe,
    Cancelled,
}

fn check(
    outcomes: Vec<SearchOutcome<Vec<SearchResult>>>,
    mode: InspectionMode,
    cancelled: bool,
    expected: Expected,
) {
    let original = SecKeychain::user_interaction_allowed().unwrap();
    let expected_searches = outcomes.len();
    let mut outcomes = outcomes.into_iter();
    let mut searches = 0;
    let mut tool_used = false;
    let mut query = ItemSearchOptions::new();
    query
        .class(ItemClass::generic_password())
        .service("ExampleProject-credentials")
        .account("fixture-user")
        .load_data(true)
        .skip_authenticated_items(true);
    let result = read_with(
        query,
        mode,
        &|| cancelled,
        |_| {
            assert!(!SecKeychain::user_interaction_allowed().unwrap());
            searches += 1;
            outcomes.next().expect("unexpected extra Keychain search")
        },
        |timeout| {
            assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);
            assert_eq!(
                timeout,
                match mode {
                    InspectionMode::Interactive => PICKER_READ_TIMEOUT,
                    InspectionMode::Headless => HEADLESS_READ_TIMEOUT,
                }
            );
            tool_used = true;
            match expected {
                Expected::MissingAfterProbe | Expected::InconclusiveAfterProbe => {
                    let mut command = std::process::Command::new("/bin/sh");
                    command.args(["-c", "exec 1>&-; sleep 0.05; exit 44"]);
                    prompt::run(&mut command, timeout, &|| false)
                }
                _ => Ok(Zeroizing::new(b"example-tool-token".to_vec())),
            }
        },
    );
    assert_eq!(searches, expected_searches);
    assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);
    assert_eq!(
        tool_used,
        matches!(
            expected,
            Expected::ToolRead | Expected::MissingAfterProbe | Expected::InconclusiveAfterProbe
        )
    );
    match expected {
        Expected::Stored => assert_eq!(&**result.unwrap().unwrap(), b"example-stored-token"),
        Expected::Missing => assert!(result.unwrap().is_none()),
        Expected::MissingAfterProbe => {
            assert!(matches!(result, Ok(None)));
            let file_error =
                crate::claude::usage::credentials::prefer_keychain(result, &|| false, 0, || {
                    Err("fixture file error".into())
                })
                .err()
                .unwrap();
            assert_eq!(file_error, "fixture file error");
        }
        Expected::InconclusiveAfterProbe => {
            assert!(matches!(result, Err(ReadError::SearchMiss)));
            let error =
                crate::claude::usage::credentials::prefer_keychain(result, &|| false, 0, || {
                    panic!("ambiguous search selected the file")
                })
                .err()
                .unwrap();
            assert!(error.contains("could not be confirmed absent"));
        }
        Expected::ToolRead => assert_eq!(&**result.unwrap().unwrap(), b"example-tool-token"),
        Expected::Cancelled => assert!(matches!(result, Err(ReadError::Cancelled))),
    }
}

fn failure(code: i32) -> SearchOutcome<Vec<SearchResult>> {
    Err(Error::from_code(code))
}

/// Native searches that leave an existing item unread without a dialog.
fn protected() -> Vec<Vec<SearchOutcome<Vec<SearchResult>>>> {
    vec![
        vec![failure(-25308)],
        vec![failure(-25293)],
        vec![failure(-25300), failure(-25308)],
        vec![failure(-25300), failure(-25293)],
        vec![failure(-25300), Ok(vec![SearchResult::Other])],
    ]
}

fn scenarios() {
    for mode in [InspectionMode::Headless, InspectionMode::Interactive] {
        check(
            vec![Ok(vec![SearchResult::Data(
                b"example-stored-token".to_vec(),
            )])],
            mode,
            false,
            Expected::Stored,
        );
        for outcomes in [
            vec![Ok(vec![]), failure(-25300)],
            vec![failure(-25300), failure(-25300)],
            vec![failure(-25300), Ok(vec![])],
        ] {
            check(outcomes, mode, false, Expected::Missing);
        }
        // Headless inspection reads through the security tool too, which
        // returns at once once the user has allowed it.
        for outcomes in protected() {
            check(outcomes, mode, false, Expected::ToolRead);
        }
        // The same tool status can mean deletion or failure to search. The
        // second attribute probe determines whether the file is authoritative.
        for (confirmation, expected) in [
            (failure(-25300), Expected::MissingAfterProbe),
            (
                Ok(vec![SearchResult::Other]),
                Expected::InconclusiveAfterProbe,
            ),
            (failure(-25308), Expected::InconclusiveAfterProbe),
        ] {
            check(
                vec![failure(-25300), Ok(vec![SearchResult::Other]), confirmation],
                mode,
                false,
                expected,
            );
        }
        check(vec![failure(-25308)], mode, true, Expected::Cancelled);
    }
}

#[test]
fn credential_reads_suppress_dialogs_and_route_missing_and_protected_items() {
    // Keep all process-wide interaction checks in one test so its explicit
    // already-disabled case cannot race another test's restoration assertions.
    scenarios();
    let original = SecKeychain::user_interaction_allowed().unwrap();
    {
        let _disabled = if original {
            Some(SecKeychain::disable_user_interaction().unwrap())
        } else {
            None
        };
        scenarios();
        assert!(!SecKeychain::user_interaction_allowed().unwrap());
    }
    assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);
}
