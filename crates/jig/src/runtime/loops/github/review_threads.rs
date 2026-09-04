const REVIEW_THREAD_COMMENT_PAGE_LIMIT: usize = 10;

pub(super) fn github_pr_review_threads_snapshot(
    ctx: &RepoContext,
    pr_number: u64,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let mut client = GithubSnapshotClient::new(ctx, observer);
    let repository = repository_snapshot(&mut client)?;
    let mut permissions = RepositoryPermissionCache::default();
    let review_threads =
        review_threads_snapshot(&mut client, &repository, pr_number, &mut permissions)?;
    let snapshot = json!({
        "review_threads": review_threads,
        "budget": client.budget_snapshot(),
    });
    require_serialized_snapshot_budget(&snapshot, GITHUB_SNAPSHOT_EVIDENCE_BYTE_LIMIT)?;
    Ok(snapshot)
}

fn review_threads_snapshot(
    client: &mut GithubSnapshotClient<'_>,
    repository: &RepositorySnapshot,
    pr_number: u64,
    permissions: &mut RepositoryPermissionCache,
) -> Result<Value> {
    let mut nodes = Vec::new();
    let mut has_next_page = true;
    let mut cursor = None;
    let mut page_count = 0;
    let mut truncated = false;
    let mut total_count = None;
    let mut thread_ids = BTreeSet::new();
    let mut cursors = BTreeSet::new();

    while has_next_page {
        if client.cancelled() {
            return Err(ExecutionCommandError::Cancelled.into_anyhow());
        }
        if page_count >= REVIEW_THREAD_PAGE_LIMIT {
            truncated = true;
            break;
        }
        page_count += 1;
        let page = review_thread_page(client, repository, pr_number, cursor.as_deref())?;
        let connection = page
            .pointer("/data/repository/pullRequest/reviewThreads")
            .ok_or_else(|| anyhow!("GitHub GraphQL response did not include reviewThreads"))?;
        let page_nodes = connection
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("GitHub GraphQL reviewThreads.nodes was not an array"))?;
        let observed_total = connection
            .get("totalCount")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("GitHub GraphQL reviewThreads.totalCount was missing"))?;
        if total_count.replace(observed_total).is_some_and(|total| total != observed_total) {
            truncated = true;
            break;
        }
        for thread in page_nodes {
            let Some(thread_id) = thread
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                truncated = true;
                break;
            };
            if !thread_ids.insert(thread_id.to_string()) {
                truncated = true;
                break;
            }
            let thread = normalize_review_thread(client, repository, thread, permissions)?;
            truncated |= thread
                .pointer("/comments/truncated")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            nodes.push(thread);
        }
        if truncated {
            break;
        }
        has_next_page = connection
            .pointer("/pageInfo/hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let next_cursor = connection
            .pointer("/pageInfo/endCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(ToOwned::to_owned);
        if has_next_page {
            let Some(next_cursor) = next_cursor.as_ref() else {
                truncated = true;
                cursor = None;
                break;
            };
            if !cursors.insert(next_cursor.clone()) {
                truncated = true;
                break;
            }
        }
        cursor = next_cursor;
    }

    truncated |= has_next_page
        || total_count.is_some_and(|total_count| total_count != nodes.len() as u64);

    Ok(json!({
        "summary": review_thread_summary(&nodes),
        "nodes": nodes,
        "page_info": {
            "page_count": page_count,
            "total_count": total_count,
            "truncated": truncated,
            "has_next_page": has_next_page,
            "end_cursor": cursor,
        },
    }))
}

fn review_thread_page(
    client: &mut GithubSnapshotClient<'_>,
    repository: &RepositorySnapshot,
    pr_number: u64,
    cursor: Option<&str>,
) -> Result<Value> {
    client.json(
        review_thread_page_args(repository, pr_number, cursor),
        &[0],
    )
}

fn review_thread_page_args(
    repository: &RepositorySnapshot,
    pr_number: u64,
    cursor: Option<&str>,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("api"),
        OsString::from("graphql"),
        OsString::from("-f"),
        OsString::from(format!("query={}", review_threads_query())),
        OsString::from("-f"),
        OsString::from(format!("owner={}", repository.owner)),
        OsString::from("-f"),
        OsString::from(format!("name={}", repository.name)),
        OsString::from("-F"),
        OsString::from(format!("number={pr_number}")),
    ];
    if let Some(cursor) = cursor {
        args.push(OsString::from("-f"));
        args.push(OsString::from(format!("threadsAfter={cursor}")));
    }
    args
}

const fn review_threads_query() -> &'static str {
    r"
query($owner: String!, $name: String!, $number: Int!, $threadsAfter: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $threadsAfter) {
        totalCount
        pageInfo {
          hasNextPage
          endCursor
        }
        nodes {
          id
          isResolved
          isOutdated
          path
          line
          startLine
          originalLine
          originalStartLine
          subjectType
          diffSide
          startDiffSide
          viewerCanReply
          viewerCanResolve
          viewerCanUnresolve
          resolvedBy {
            login
          }
          comments(last: 100) {
            totalCount
            pageInfo {
              hasPreviousPage
              startCursor
            }
            nodes {
              id
              url
              body
              createdAt
              updatedAt
              viewerDidAuthor
              author {
                login
              }
            }
          }
        }
      }
    }
  }
}
"
}

struct ReviewThreadComments {
    nodes: Vec<Value>,
    total_count: u64,
    page_count: usize,
    truncated: bool,
}

fn review_thread_comments(
    client: &mut GithubSnapshotClient<'_>,
    thread: &Value,
) -> Result<ReviewThreadComments> {
    let thread_id = thread
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow!("GitHub review thread did not include an id"))?;
    let connection = thread
        .get("comments")
        .ok_or_else(|| anyhow!("GitHub review thread did not include comments"))?;
    let total_count = connection
        .get("totalCount")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("GitHub review thread did not include comments.totalCount"))?;
    let mut nodes = connection
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("GitHub review thread comments.nodes was not an array"))?
        .clone();
    let mut has_previous_page = connection
        .pointer("/pageInfo/hasPreviousPage")
        .and_then(Value::as_bool)
        .unwrap_or(total_count > nodes.len() as u64);
    let mut cursor = connection
        .pointer("/pageInfo/startCursor")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map(ToOwned::to_owned);
    let mut page_count = 1;
    let mut truncated = false;

    while has_previous_page {
        if client.cancelled() {
            return Err(ExecutionCommandError::Cancelled.into_anyhow());
        }
        if page_count >= REVIEW_THREAD_COMMENT_PAGE_LIMIT {
            truncated = true;
            break;
        }
        let Some(before) = cursor.as_deref() else {
            truncated = true;
            break;
        };
        let page = review_thread_comment_page(client, thread_id, before)?;
        let observed_id = page.pointer("/data/node/id").and_then(Value::as_str);
        let comments = page
            .pointer("/data/node/comments")
            .ok_or_else(|| anyhow!("GitHub review thread comment page omitted comments"))?;
        let observed_total = comments
            .get("totalCount")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("GitHub review thread comment page omitted totalCount"))?;
        if observed_id != Some(thread_id) || observed_total != total_count {
            truncated = true;
            break;
        }
        let older = comments
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("GitHub review thread comment page nodes was not an array"))?;
        nodes.splice(0..0, older.iter().cloned());
        page_count += 1;
        has_previous_page = comments
            .pointer("/pageInfo/hasPreviousPage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        cursor = comments
            .pointer("/pageInfo/startCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(ToOwned::to_owned);
    }

    let comment_ids = nodes
        .iter()
        .filter_map(|comment| comment.get("id").and_then(Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    truncated |= has_previous_page
        || total_count != nodes.len() as u64
        || comment_ids.len() != nodes.len();
    Ok(ReviewThreadComments {
        nodes,
        total_count,
        page_count,
        truncated,
    })
}

fn review_thread_comment_page(
    client: &mut GithubSnapshotClient<'_>,
    thread_id: &str,
    before: &str,
) -> Result<Value> {
    client.json(
        vec![
            OsString::from("api"),
            OsString::from("graphql"),
            OsString::from("-f"),
            OsString::from(format!("query={}", review_thread_comments_query())),
            OsString::from("-f"),
            OsString::from(format!("threadId={thread_id}")),
            OsString::from("-f"),
            OsString::from(format!("commentsBefore={before}")),
        ],
        &[0],
    )
}

const fn review_thread_comments_query() -> &'static str {
    r"
query($threadId: ID!, $commentsBefore: String) {
  node(id: $threadId) {
    ... on PullRequestReviewThread {
      id
      comments(last: 100, before: $commentsBefore) {
        totalCount
        pageInfo {
          hasPreviousPage
          startCursor
        }
        nodes {
          id
          url
          body
          createdAt
          updatedAt
          viewerDidAuthor
          author {
            login
          }
        }
      }
    }
  }
}
"
}

fn normalize_review_thread(
    client: &mut GithubSnapshotClient<'_>,
    repository: &RepositorySnapshot,
    thread: &Value,
    permissions: &mut RepositoryPermissionCache,
) -> Result<Value> {
    let comments_snapshot = review_thread_comments(client, thread)?;
    client.reserve_review_items(1_usize.saturating_add(comments_snapshot.nodes.len()))?;
    let mut comments = Vec::new();
    for comment in &comments_snapshot.nodes {
        let login = comment.pointer("/author/login").and_then(Value::as_str);
        let author = if comments_snapshot.truncated {
            untrusted_author_snapshot(login)
        } else {
            permissions.author_snapshot(client, repository, login)?
        };
        comments.push(json!({
            "id": comment.get("id").cloned().unwrap_or(Value::Null),
            "url": comment.get("url").cloned().unwrap_or(Value::Null),
            "body": comment.get("body").cloned().unwrap_or(Value::Null),
            "createdAt": comment.get("createdAt").cloned().unwrap_or(Value::Null),
            "updatedAt": comment.get("updatedAt").cloned().unwrap_or(Value::Null),
            "viewerDidAuthor": comment.get("viewerDidAuthor").cloned().unwrap_or(Value::Null),
            "author": author,
        }));
    }
    let has_trusted_comment = !comments_snapshot.truncated
        && comments.iter().any(|comment| {
            comment
                .pointer("/author/trusted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
    Ok(json!({
        "id": thread.get("id").cloned().unwrap_or(Value::Null),
        "is_resolved": thread.get("isResolved").cloned().unwrap_or(Value::Null),
        "is_outdated": thread.get("isOutdated").cloned().unwrap_or(Value::Null),
        "path": thread.get("path").cloned().unwrap_or(Value::Null),
        "line": thread.get("line").cloned().unwrap_or(Value::Null),
        "start_line": thread.get("startLine").cloned().unwrap_or(Value::Null),
        "original_line": thread.get("originalLine").cloned().unwrap_or(Value::Null),
        "original_start_line": thread.get("originalStartLine").cloned().unwrap_or(Value::Null),
        "subject_type": thread.get("subjectType").cloned().unwrap_or(Value::Null),
        "diff_side": thread.get("diffSide").cloned().unwrap_or(Value::Null),
        "start_diff_side": thread.get("startDiffSide").cloned().unwrap_or(Value::Null),
        "viewer_can_reply": thread.get("viewerCanReply").cloned().unwrap_or(Value::Null),
        "viewer_can_resolve": thread.get("viewerCanResolve").cloned().unwrap_or(Value::Null),
        "viewer_can_unresolve": thread.get("viewerCanUnresolve").cloned().unwrap_or(Value::Null),
        "resolved_by": {
            "login": thread.pointer("/resolvedBy/login").cloned().unwrap_or(Value::Null),
        },
        "comments": {
            "total_count": comments_snapshot.total_count,
            "page_count": comments_snapshot.page_count,
            "truncated": comments_snapshot.truncated,
            "nodes": comments,
        },
        "has_trusted_comment": has_trusted_comment,
    }))
}
