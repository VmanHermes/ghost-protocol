use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn cors_layer(request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get("origin")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("*"));

    if request.method() == axum::http::Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        apply_cors_headers(response.headers_mut(), origin);
        return response;
    }

    let mut response = next.run(request).await;
    apply_cors_headers(response.headers_mut(), origin);
    response
}

fn apply_cors_headers(headers: &mut axum::http::HeaderMap, origin: HeaderValue) {
    headers.insert("access-control-allow-origin", origin);
    headers.insert(
        "vary",
        HeaderValue::from_static("Origin"),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static("Content-Type, Authorization"),
    );
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
}
