use super::*;

pub(super) fn assert_required_coverage(
    config: &toml::Value,
    native: bool,
    language: BackendLanguage,
) {
    let gates = config["work"]["gates"].as_array().unwrap();
    let mut required = vec![
        ("api", "test", "jig.test"),
        ("repo", "contract", "jig.contract_check"),
        ("web", "lint", "jig.typescript_lint"),
        ("web", "typecheck", "jig.typescript_typecheck"),
        ("web", "build", "jig.typescript_build"),
        ("web", "test", "jig.typescript_coverage"),
    ];
    if language == BackendLanguage::Rust {
        required.extend([
            ("api", "sqlx", "jig.sqlx_check"),
            ("api", "schema", "jig.schema_check"),
        ]);
    } else {
        required.push(("api", "sqlc", "jig.sqlc_check"));
    }
    if native {
        let default = &config["repository"]["default_check_profile"];
        assert_eq!(&gates[0]["profile"], default);
        let profile = config["repository"]["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|profile| &profile["id"] == default)
            .unwrap();
        required.extend([("api", "fmt", ""), ("repo", "file-budget", "")]);
        required.push((
            "api",
            if language == BackendLanguage::Rust {
                "clippy"
            } else {
                "lint"
            },
            "",
        ));
        for (component, action, _) in required {
            assert!(
                profile["targets"].as_array().unwrap().iter().any(|target| {
                    target["component"].as_str() == Some(component)
                        && target["action"].as_str() == Some(action)
                }),
                "missing {component}:{action}"
            );
        }
        assert!(
            !profile["targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|target| { target["action"].as_str() == Some("schema-dump") })
        );
    } else {
        if language == BackendLanguage::Rust {
            required.push(("api", "schema-dump", "jig.schema_dump"));
        }
        for (_, _, tool) in required {
            assert!(
                gates.iter().any(|gate| gate["tool"].as_str() == Some(tool)),
                "missing {tool}"
            );
        }
    }
}
