use security_framework::base::{Error, Result as SearchOutcome};
use security_framework::os::macos::keychain::SecKeychain;

use super::*;

enum Expected {
    Stored,
    Missing,
    Prompted,
    PermissionRequired,
    Cancelled,
}

fn check(
    outcomes: Vec<SearchOutcome<Vec<SearchResult>>>,
    allow_prompt: bool,
    cancelled: bool,
    expected: Expected,
) {
    let original = SecKeychain::user_interaction_allowed().unwrap();
    let expected_searches = outcomes.len();
    let mut outcomes = outcomes.into_iter();
    let mut searches = 0;
    let mut prompted = false;
    let mut query = ItemSearchOptions::new();
    query
        .class(ItemClass::generic_password())
        .service("ExampleProject-credentials")
        .account("fixture-user")
        .load_data(true)
        .skip_authenticated_items(true);
    let result = read_with(
        query,
        allow_prompt,
        &|| cancelled,
        |_| {
            assert!(!SecKeychain::user_interaction_allowed().unwrap());
            searches += 1;
            outcomes.next().expect("unexpected extra Keychain search")
        },
        || {
            assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);
            prompted = true;
            Ok(Zeroizing::new(b"example-prompt-token".to_vec()))
        },
    );
    assert_eq!(searches, expected_searches);
    assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), original);
    assert_eq!(prompted, matches!(expected, Expected::Prompted));
    match expected {
        Expected::Stored => assert_eq!(&**result.unwrap().unwrap(), b"example-stored-token"),
        Expected::Missing => assert!(result.unwrap().is_none()),
        Expected::Prompted => assert_eq!(&**result.unwrap().unwrap(), b"example-prompt-token"),
        Expected::PermissionRequired => assert!(result.unwrap_err().contains("require permission")),
        Expected::Cancelled => assert!(result.unwrap_err().contains("cancelled")),
    }
}

fn failure(code: i32) -> SearchOutcome<Vec<SearchResult>> {
    Err(Error::from_code(code))
}

fn scenarios() {
    for allow_prompt in [false, true] {
        check(
            vec![Ok(vec![SearchResult::Data(
                b"example-stored-token".to_vec(),
            )])],
            allow_prompt,
            false,
            Expected::Stored,
        );
        for outcomes in [
            vec![Ok(vec![])],
            vec![failure(-25300), failure(-25300)],
            vec![failure(-25300), Ok(vec![])],
        ] {
            check(outcomes, allow_prompt, false, Expected::Missing);
        }
        for outcomes in [
            vec![failure(-25308)],
            vec![failure(-25293)],
            vec![failure(-25300), failure(-25308)],
            vec![failure(-25300), failure(-25293)],
            vec![failure(-25300), Ok(vec![SearchResult::Other])],
        ] {
            check(
                outcomes,
                allow_prompt,
                false,
                if allow_prompt {
                    Expected::Prompted
                } else {
                    Expected::PermissionRequired
                },
            );
        }
        check(
            vec![failure(-25308)],
            allow_prompt,
            true,
            Expected::Cancelled,
        );
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
