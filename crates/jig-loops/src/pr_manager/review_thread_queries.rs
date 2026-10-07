const fn add_review_thread_reply_mutation() -> &'static str {
    r"
mutation($threadId: ID!, $body: String!) {
  addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $threadId, body: $body}) {
    comment {
      id
      url
    }
  }
}
"
}

const fn resolve_review_thread_mutation() -> &'static str {
    r"
mutation($threadId: ID!) {
  resolveReviewThread(input: {threadId: $threadId}) {
    thread {
      id
      isResolved
    }
  }
}
"
}

const fn review_thread_reply_state_query() -> &'static str {
    r"
query ReviewThreadState($threadId: ID!, $commentsBefore: String) {
  node(id: $threadId) {
    ... on PullRequestReviewThread {
      id
      comments(last: 100, before: $commentsBefore) {
        pageInfo {
          hasPreviousPage
          startCursor
        }
        nodes {
          id
          url
          body
          viewerDidAuthor
        }
      }
    }
  }
}
"
}

const fn review_thread_resolution_state_query() -> &'static str {
    r"
query ReviewThreadState($threadId: ID!) {
  node(id: $threadId) {
    ... on PullRequestReviewThread {
      id
      isResolved
      pullRequest {
        headRefOid
      }
      comments(last: 1) {
        totalCount
        nodes {
          id
        }
      }
    }
  }
}
"
}

const fn review_thread_witness_state_query() -> &'static str {
    r"
query ReviewThreadWitnessState($threadId: ID!, $commentsBefore: String) {
  node(id: $threadId) {
    ... on PullRequestReviewThread {
      id
      isResolved
      pullRequest {
        headRefOid
      }
      comments(last: 100, before: $commentsBefore) {
        totalCount
        pageInfo {
          hasPreviousPage
          startCursor
        }
        nodes {
          id
          updatedAt
          body
        }
      }
    }
  }
}
"
}
