use security_framework::base::{Error, Result as SearchOutcome};
use security_framework::os::macos::keychain::SecKeychain;

use super::*;

enum Expected {
    Stored,
    Missing,
    ToolRead,
    PermissionRequired,
    ToolDenied,
    Cancelled,
}

fn check(
    outcomes: Vec<SearchOutcome<Vec<SearchResult>>>,
    mode: InspectionMode,
    cancelled: bool,
    tool_reads: bool,
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
            if tool_reads {
                Ok(Zeroizing::new(b"example-tool-token".to_vec()))
            } else {
                Err("Claude Keychain access was denied or unavailable".into())
            }
        },
    );
    assert_eq!(searches, expected_searches);
    assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);
    assert_eq!(
        tool_used,
        matches!(
            expected,
            Expected::ToolRead | Expected::PermissionRequired | Expected::ToolDenied
        )
    );
    match expected {
        Expected::Stored => assert_eq!(&**result.unwrap().unwrap(), b"example-stored-token"),
        Expected::Missing => assert!(result.unwrap().is_none()),
        Expected::ToolRead => assert_eq!(&**result.unwrap().unwrap(), b"example-tool-token"),
        Expected::PermissionRequired => assert!(result.unwrap_err().contains("require permission")),
        Expected::ToolDenied => assert!(result.unwrap_err().contains("denied")),
        Expected::Cancelled => assert!(result.unwrap_err().contains("cancelled")),
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
            true,
            Expected::Stored,
        );
        for outcomes in [
            vec![Ok(vec![])],
            vec![failure(-25300), failure(-25300)],
            vec![failure(-25300), Ok(vec![])],
        ] {
            check(outcomes, mode, false, true, Expected::Missing);
        }
        // Headless inspection reads through the security tool too, which
        // returns at once once the user has allowed it.
        for outcomes in protected() {
            check(outcomes, mode, false, true, Expected::ToolRead);
        }
        for outcomes in protected() {
            check(
                outcomes,
                mode,
                false,
                false,
                match mode {
                    InspectionMode::Headless => Expected::PermissionRequired,
                    InspectionMode::Interactive => Expected::ToolDenied,
                },
            );
        }
        check(vec![failure(-25308)], mode, true, true, Expected::Cancelled);
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
