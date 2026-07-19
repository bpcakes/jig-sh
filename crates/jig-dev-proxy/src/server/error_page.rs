use askama::Template;
use bytes::Bytes;
use hyper::header::HeaderValue;
use hyper::{Response, StatusCode};

use crate::types::Route;

use super::ProxyBody;
use super::headers::full_body;

#[derive(Template)]
#[template(path = "error_page.html")]
struct ErrorPageTemplate<'a> {
    code: u16,
    title_lead: &'static str,
    title_hot: &'static str,
    reason: &'static str,
    message: &'a str,
    requested_host: &'a str,
    is_not_found: bool,
    show_routes_section: bool,
    show_route_notice: bool,
    route_notice: &'static str,
    routes: Vec<ErrorRoute>,
}

struct ErrorRoute {
    index: String,
    hostname: String,
    url: String,
}

pub(super) fn not_found_response(
    routes: &[Route],
    host: &str,
    proxy_port: u16,
    tls: bool,
    show_routes: bool,
) -> Response<ProxyBody> {
    let scheme = if tls { "https" } else { "http" };
    let route_links = if show_routes {
        routes
            .iter()
            .enumerate()
            .map(|(index, route)| ErrorRoute {
                index: format!("{:02}", index + 1),
                hostname: route.hostname.to_string(),
                url: format!("{scheme}://{}:{proxy_port}", route.hostname),
            })
            .collect()
    } else {
        Vec::new()
    };
    let route_notice = if !show_routes {
        "Route listing is hidden for non-loopback clients."
    } else if routes.is_empty() {
        "No apps running. Start the repo development command, then reload this page."
    } else {
        ""
    };
    let template = error_template(StatusCode::NOT_FOUND, "", host, route_links);
    html_error_response(
        StatusCode::NOT_FOUND,
        ErrorPageTemplate {
            show_routes_section: true,
            show_route_notice: !route_notice.is_empty(),
            route_notice,
            ..template
        },
    )
}

pub(super) fn error_response(status: StatusCode, message: &str) -> Response<ProxyBody> {
    html_error_response(status, error_template(status, message, "", Vec::new()))
}

fn error_template<'a>(
    status: StatusCode,
    message: &'a str,
    requested_host: &'a str,
    routes: Vec<ErrorRoute>,
) -> ErrorPageTemplate<'a> {
    let (title_lead, title_hot) = error_title(status);
    ErrorPageTemplate {
        code: status.as_u16(),
        title_lead,
        title_hot,
        reason: status.canonical_reason().unwrap_or("Proxy error"),
        message,
        requested_host,
        is_not_found: status == StatusCode::NOT_FOUND,
        show_routes_section: false,
        show_route_notice: false,
        route_notice: "",
        routes,
    }
}

fn html_error_response(status: StatusCode, template: ErrorPageTemplate<'_>) -> Response<ProxyBody> {
    // The template is compiled and all interpolated values use infallible
    // standard Display implementations, so rendering into a String cannot
    // fail at runtime.
    let html = template
        .render()
        .expect("compiled Jig proxy error template must render");
    let mut response = Response::new(full_body(Bytes::from(html)));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-jig-proxy", HeaderValue::from_static("1"));
    response
}

fn error_title(status: StatusCode) -> (&'static str, &'static str) {
    match status {
        StatusCode::BAD_REQUEST => ("BAD", "REQUEST."),
        StatusCode::FORBIDDEN => ("ACCESS", "DENIED."),
        StatusCode::NOT_FOUND => ("ROUTE", "NOT FOUND."),
        StatusCode::METHOD_NOT_ALLOWED => ("METHOD", "NOT ALLOWED."),
        StatusCode::PAYLOAD_TOO_LARGE => ("PAYLOAD", "TOO LARGE."),
        StatusCode::BAD_GATEWAY => ("BAD", "GATEWAY."),
        StatusCode::SERVICE_UNAVAILABLE => ("PROXY", "BUSY."),
        _ => ("PROXY", "ERROR."),
    }
}
