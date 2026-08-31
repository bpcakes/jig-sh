use std::collections::BTreeMap;

#[derive(Default)]
struct RepositoryPermissionCache {
    by_login: BTreeMap<String, Value>,
}

impl RepositoryPermissionCache {
    fn author_snapshot(
        &mut self,
        ctx: &RepoContext,
        repository: &RepositorySnapshot,
        login: Option<&str>,
        observer: &mut dyn ExecutionControl,
    ) -> Result<Value> {
        let Some(login) = login.filter(|login| !login.is_empty()) else {
            return Ok(json!({
                "login": Value::Null,
                "permission": Value::Null,
                "trusted": false,
            }));
        };
        if let Some(cached) = self.by_login.get(login) {
            return Ok(cached.clone());
        }
        let encoded_login = encode_path_segment(login);
        let endpoint = format!(
            "repos/{}/{}/collaborators/{encoded_login}/permission",
            repository.owner, repository.name
        );
        let output = run_gh(
            ctx,
            vec![
                OsString::from("api"),
                OsString::from("--method"),
                OsString::from("GET"),
                OsString::from(endpoint),
            ],
            observer,
        )?;
        let permission = match output.status_code {
            Some(0) => parse_gh_json(&output.stdout, "gh collaborator permission")?
                .get("permission")
                .and_then(Value::as_str)
                .map(str::to_string),
            Some(1) if output.stderr.contains("HTTP 404") => None,
            _ => return Err(output.into_error("gh collaborator permission")),
        };
        let snapshot = json!({
            "login": login,
            "permission": permission,
            "trusted": permission.as_deref().is_some_and(permission_is_trusted),
        });
        self.by_login.insert(login.to_string(), snapshot.clone());
        Ok(snapshot)
    }
}

fn permission_is_trusted(permission: &str) -> bool {
    matches!(permission, "admin" | "write")
}

fn encode_path_segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}
