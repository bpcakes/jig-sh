use security_framework::base::{Error, Result as SearchOutcome};
use security_framework::os::macos::keychain::SecKeychain;

use super::*;

enum Expected {
    Stored,
    Missing,
    ToolRead,
    Unavailable,
    Refused,
    StillWaiting,
    ToolUnreadable,
    Cancelled,
}

fn check(
    outcomes: Vec<SearchOutcome<Vec<SearchResult>>>,
    mode: InspectionMode,
    cancelled: bool,
    tool_failure: Option<ToolFailure>,
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
            match tool_failure {
                None => Ok(Zeroizing::new(b"example-tool-token".to_vec())),
                Some(failure) => Err(failure),
            }
        },
    );
    assert_eq!(searches, expected_searches);
    assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);
    assert_eq!(
        tool_used,
        matches!(
            expected,
            Expected::ToolRead
                | Expected::Unavailable
                | Expected::Refused
                | Expected::StillWaiting
                | Expected::ToolUnreadable
        )
    );
    match expected {
        Expected::Stored => assert_eq!(&**result.unwrap().unwrap(), b"example-stored-token"),
        Expected::Missing => assert!(result.unwrap().is_none()),
        Expected::ToolRead => assert_eq!(&**result.unwrap().unwrap(), b"example-tool-token"),
        expected => assert_read_error(result.unwrap_err(), mode, expected),
    }
}

/// Checks whether an unread item lets the file login stand in, and its message.
fn assert_read_error(error: ReadError, mode: InspectionMode, expected: Expected) {
    let interactive = matches!(mode, InspectionMode::Interactive);
    let (file_fallback, summary, remedies) = match expected {
        // A session that cannot show a dialog, as over SSH, would leave a
        // launch without the item too, so the file login may stand in.
        Expected::Unavailable if interactive => (true, "cannot ask for permission", true),
        Expected::Unavailable => (true, "needs permission", true),
        // A Deny or cancelled dialog answers one request: a launch asks again
        // and may then use the Keychain login.
        Expected::Refused => (false, "denied", true),
        // A launch would wait on the dialog and use the Keychain login, so
        // the file login must not stand in for it.
        Expected::StillWaiting if interactive => (false, "timed out", true),
        Expected::StillWaiting => (false, "needs permission", true),
        // Oversized or unreadable output, or a tool left running, is an item a
        // launch would still read.
        Expected::ToolUnreadable => (false, "could not be read", false),
        Expected::Cancelled => (false, "cancelled", false),
        Expected::Stored | Expected::Missing | Expected::ToolRead => {
            unreachable!("a read expected to succeed failed: {}", error.message)
        }
    };
    assert_eq!(error.file_fallback, file_fallback, "{}", error.message);
    assert!(error.message.contains(summary), "{}", error.message);
    if remedies {
        assert_names_both_remedies(&error.message);
    }
}

/// Unlocking does not grant item permission, so both modes name each remedy.
fn assert_names_both_remedies(error: &str) {
    assert!(error.contains("security unlock-keychain"), "{error}");
    assert!(
        error.contains("Always Allow") && error.contains("jig claude launch"),
        "{error}"
    );
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
            None,
            Expected::Stored,
        );
        for outcomes in [
            vec![Ok(vec![])],
            vec![failure(-25300), failure(-25300)],
            vec![failure(-25300), Ok(vec![])],
        ] {
            check(outcomes, mode, false, None, Expected::Missing);
        }
        // Headless inspection reads through the security tool too, which
        // returns at once once the user has allowed it.
        for outcomes in protected() {
            check(outcomes, mode, false, None, Expected::ToolRead);
        }
        for outcomes in protected() {
            check(
                outcomes,
                mode,
                false,
                Some(ToolFailure::Unavailable),
                Expected::Unavailable,
            );
        }
        for outcomes in protected() {
            check(
                outcomes,
                mode,
                false,
                Some(ToolFailure::Refused),
                Expected::Refused,
            );
        }
        for outcomes in protected() {
            check(
                outcomes,
                mode,
                false,
                Some(ToolFailure::Waiting),
                Expected::StillWaiting,
            );
        }
        for outcomes in protected() {
            check(
                outcomes,
                mode,
                false,
                Some(ToolFailure::Unreadable),
                Expected::ToolUnreadable,
            );
        }
        check(vec![failure(-25308)], mode, true, None, Expected::Cancelled);
    }
}

#[test]
fn security_tool_exits_fall_back_only_when_a_launch_would_lack_the_item() {
    let unavailable = |code| matches!(prompt::ended_without_item(code), ToolFailure::Unavailable);
    // errSecInteractionNotAllowed, as over SSH, and errSecItemNotFound.
    assert!(unavailable(Some(36)));
    assert!(unavailable(Some(44)));
    // errSecAuthFailed after a Deny, errSecUserCanceled, other failures, and a
    // tool stopped by a signal.
    for code in [Some(51), Some(128), Some(1), None] {
        assert!(!unavailable(code), "{code:?}");
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
