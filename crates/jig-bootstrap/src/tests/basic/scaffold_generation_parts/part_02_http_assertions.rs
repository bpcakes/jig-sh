fn assert_admin_http_crate(destination: &Path) {
    let admin_http_lib =
        fs::read_to_string(destination.join("crates/my-app-admin-http/src/lib.rs")).unwrap();
    let authorization =
        fs::read_to_string(destination.join("crates/my-app-admin-http/src/authorization.rs"))
            .unwrap();
    let admin_tests =
        fs::read_to_string(destination.join("crates/my-app-admin-http/src/tests.rs")).unwrap();
    assert_contains_all(
        &authorization,
        &[
            "pub trait AdminAuthorizer",
            "pub struct DenyAllAdminAuthorizer",
            "admitted: AdmittedRequest,",
            "let correlation_id = admitted.correlation_id();",
            "ApiError::unauthorized(Some(correlation_id))",
            "ApiError::forbidden(Some(correlation_id))",
        ],
    );
    assert_contains_all(
        &admin_http_lib,
        &[
            "pub use authorization::{",
            "pub async fn assemble<A, E>(",
            "authorization::require_admin_authorization::<A>",
            "GuardedRouter::from_router(protected, inventory)",
            "GroupPolicy::browser(requests::policy(admission), browser_policy()?)",
            ".with_probe_response_policy(PrivateResponsePolicy::NoReferrer)",
            "MutationPolicy::required_header(marker)",
            "PrivateResponsePolicy::NoReferrer,",
            "pub const ADMIN_REQUEST_HEADER: &str = \"x-admin-request\";",
            "pub async fn in_process<A: AdminAuthorizer>(",
            "pub fn openapi() -> OpenApiDocument",
            "components(schemas(ApiErrorResponse))",
            r#"path = "/admin-api/status""#,
            "operation_id = \"getAdminStatus\"",
        ],
    );
    assert_contains_all(
        &admin_tests,
        &[
            "admin_status_is_protected_and_reflects_readiness_after_authorization",
            "let expected_ready = state.is_ready();",
            "assert_eq!(json(response).await[\"ready\"], expected_ready);",
            "unknown_admin_paths_remain_not_found_without_authorization_challenge",
            "probes_answer_outside_authorization_with_private_headers",
            "mutations_require_same_origin_fetch_metadata_and_the_admin_marker",
            "committed_admin_openapi_is_current",
        ],
    );
    for source in [&admin_http_lib, &authorization] {
        assert_contains_none(
            source,
            &[
                "REQUEST_ID_HEADER",
                "SetRequestIdLayer",
                "PropagateRequestIdLayer",
                "router_with_shutdown",
                "router_with_lifecycle",
                "observe_http",
                "low_level",
                "ApiError::unauthorized(request.headers())",
                "ApiError::forbidden(request.headers())",
            ],
        );
    }
    let admin_api_main =
        fs::read_to_string(destination.join("apps/my-app-admin-api/src/main.rs")).unwrap();
    assert_contains_all(
        &admin_api_main,
        &[
            "runtime::serve(config, |state, admission, readiness| {",
            concat!(
                "admin_http_crate::assemble(\n",
                "            state,\n",
                "            admin_http_crate::DenyAllAdminAuthorizer,\n",
                "            admission,\n",
                "            readiness,\n",
                "        )"
            ),
        ],
    );
    assert_contains_none(&admin_api_main, &["router_with_shutdown", "serve_with_jobs"]);
}

fn assert_public_http_contract(destination: &Path) {
    let http_common_lib =
        fs::read_to_string(destination.join("crates/my-app-http-common/src/lib.rs")).unwrap();
    assert!(http_common_lib.contains("pub struct ApiErrorResponse"));
    assert!(http_common_lib.contains("pub request_id: String"));
    assert_contains_all(
        &http_common_lib,
        &[
            "use batter::axum::{AdmittedRequest, CorrelationId, RouteInventory, RouteInventoryError};",
            "correlation_id: Option<&CorrelationId>",
            "request_id: request_id(correlation_id)",
            "pub async fn not_found(admitted: AdmittedRequest) -> ApiError",
            "ApiError::not_found(Some(admitted.correlation_id()))",
            "pub fn route_inventory(document: &OpenApi) -> Result<RouteInventory, RouteInventoryError>",
            "fn request_id(correlation_id: Option<&CorrelationId>) -> String",
            ".map(CorrelationId::as_str)",
        ],
    );
    assert_contains_none(
        &http_common_lib,
        &[
            "REQUEST_ID_HEADER",
            "HeaderMap",
            "request_id(headers",
            "request.headers()",
            "Extension(",
        ],
    );
    let requests =
        fs::read_to_string(destination.join("crates/my-app-http-common/src/requests.rs")).unwrap();
    assert_contains_all(
        &requests,
        &[
            "pub const REQUEST_BUDGET: Duration = Duration::from_secs(10);",
            "ResponseConstructionBudget::new(REQUEST_BUDGET)",
            "RequestPolicy::new(admission, request_budget).with_failure_renderer(|failure, parts| {",
            concat!(
                "crate::ApiError::new(\n",
                "            failure.status(),\n",
                "            failure.code(),\n",
                "            \"The request could not be completed\",\n",
                "            parts.extensions.get::<CorrelationId>(),\n",
                "        )"
            ),
            "HttpBoundary::new(policy(shutdown.operation_admission()))",
            ".in_process();",
        ],
    );
    assert_contains_none(
        &requests,
        &[
            "RequestPolicy::new(shutdown",
            "request_admission",
            "operational_http",
            "middleware::from_fn",
        ],
    );
    let probes =
        fs::read_to_string(destination.join("crates/my-app-http-common/src/probes.rs")).unwrap();
    assert_contains_all(
        &probes,
        &[
            "pub const LIVENESS_PATH: &str = \"/health/live\";",
            "pub const READINESS_PATH: &str = \"/health/ready\";",
            "pub fn liveness(_parts: &Parts) -> Response",
            "pub fn readiness(decision: ReadinessDecision, parts: &Parts) -> Response",
            "ReadinessUnreadyReason::Dependency(_) | ReadinessUnreadyReason::Condition(_)",
            "\"dependency_unavailable\"",
            "\"service_unavailable\"",
            ".with_rendered_liveness(ProbePath::new(LIVENESS_PATH)?, liveness)?",
            ".with_rendered_readiness(ProbePath::new(READINESS_PATH)?, readiness_policy, readiness)?",
        ],
    );
    let public_http =
        fs::read_to_string(destination.join("crates/my-app-http/src/public.rs")).unwrap();
    for handler in ["health", "live", "ready", "version", "status"] {
        assert!(public_http.contains(&format!(".routes(routes!({handler}))")));
    }
    assert_contains_all(
        &public_http,
        &[
            r#"path = "/health/live""#,
            r#"path = "/health/ready""#,
            r#"path = "/api/version""#,
            r#"path = "/api/status""#,
            "body = ApiErrorResponse",
            "pub(super) const HEALTH_PATH: &str = \"/health\";",
            "pub(super) fn application_routes() -> OpenApiRouter<AppState>",
            "fn probe_documentation() -> OpenApiRouter<AppState>",
        ],
    );
    assert_contains_none(
        &public_http,
        &["HeaderMap", "ShutdownHandle", "&headers", "Extension(", "LifecycleStatus"],
    );
}
