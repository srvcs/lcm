use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::{OpenApi, ToSchema};

use crate::client::{self, DepError};

pub const SERVICE: &str = "srvcs-lcm";
pub const CONCERN: &str = "number theory: least common multiple";
pub const DEPENDS_ON: &[&str] = &["srvcs-gcd", "srvcs-divide", "srvcs-multiply"];

/// Dependency endpoints, injected as router state so tests can point them at
/// mock services.
#[derive(Clone)]
pub struct Deps {
    pub gcd_url: String,
    pub divide_url: String,
    pub multiply_url: String,
}

#[derive(Serialize, ToSchema)]
pub struct Info {
    pub service: &'static str,
    pub concern: &'static str,
    pub depends_on: Vec<&'static str>,
}

/// `GET /` — service identity (srvcs service standard).
#[utoipa::path(get, path = "/", responses((status = 200, body = Info)))]
pub async fn index() -> Json<Info> {
    Json(Info {
        service: SERVICE,
        concern: CONCERN,
        depends_on: DEPENDS_ON.to_vec(),
    })
}

#[derive(Deserialize, ToSchema)]
pub struct EvalRequest {
    pub a: i64,
    pub b: i64,
}

#[derive(Serialize, ToSchema)]
pub struct LcmResponse {
    pub a: i64,
    pub b: i64,
    pub result: i64,
}

fn ok(a: i64, b: i64, result: i64) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "a": a, "b": b, "result": result })),
    )
        .into_response()
}

fn degraded(dependency: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "dependency unavailable", "dependency": dependency })),
    )
        .into_response()
}

fn forward(status: u16, body: Value) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    (code, Json(body)).into_response()
}

/// A reachable dependency answered `200` but its body lacked an integer
/// `result`. That is a contract violation we cannot recover from, so surface a
/// `500` rather than guessing.
fn malformed(dependency: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(
            json!({ "error": "dependency returned a malformed result", "dependency": dependency }),
        ),
    )
        .into_response()
}

/// Call one dependency at `url` with `body`, mapping its outcome to either the
/// parsed response body (on `200`) or an early-return `Response` the caller
/// should surface verbatim:
///
/// - unreachable / non-`200`/`422` -> `503` degraded
/// - `422` -> forwarded `422` (the dependency rejected the input)
async fn ask(url: &str, body: &Value, dependency: &str) -> Result<Value, Response> {
    match client::call(url, body).await {
        Err(DepError::Unreachable) => Err(degraded(dependency)),
        Ok((200, body)) => Ok(body),
        Ok((422, body)) => Err(forward(422, body)),
        Ok(_) => Err(degraded(dependency)),
    }
}

/// `POST /` — compute `lcm(a, b)` by composing three primitives.
///
/// This service owns the *control flow* but delegates every arithmetic step to
/// its dependencies, exactly as specified:
///
/// 1. ask `srvcs-gcd` for `g = gcd(a, b)`;
/// 2. if `g == 0`, the result is `0` (no further calls);
/// 3. otherwise ask `srvcs-divide` for `q = a / g`, then `srvcs-multiply` for
///    `result = q * b` — i.e. `lcm = (a / gcd(a, b)) * b`.
///
/// If a dependency is unreachable it reports itself degraded (`503`); if a
/// dependency rejects the input it forwards the `422`.
#[utoipa::path(
    post,
    path = "/",
    request_body = EvalRequest,
    responses(
        (status = 200, body = LcmResponse),
        (status = 422, description = "a dependency rejected the input (forwarded)"),
        (status = 500, description = "a dependency returned a malformed result"),
        (status = 503, description = "a dependency is unavailable")
    )
)]
pub async fn evaluate(State(deps): State<Deps>, Json(req): Json<EvalRequest>) -> Response {
    let (a, b) = (req.a, req.b);

    // 1. g = gcd(a, b)
    let gcd_body = match ask(&deps.gcd_url, &json!({ "a": a, "b": b }), "srvcs-gcd").await {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let g = match gcd_body.get("result").and_then(Value::as_i64) {
        Some(g) => g,
        None => return malformed("srvcs-gcd"),
    };

    // 2. gcd(0, 0) == 0 -> lcm is 0; short-circuit without more calls.
    if g == 0 {
        return ok(a, b, 0);
    }

    // 3a. q = a / g
    let divide_body = match ask(&deps.divide_url, &json!({ "a": a, "b": g }), "srvcs-divide").await
    {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let q = match divide_body.get("result").and_then(Value::as_i64) {
        Some(q) => q,
        None => return malformed("srvcs-divide"),
    };

    // 3b. result = q * b
    let multiply_body = match ask(
        &deps.multiply_url,
        &json!({ "a": q, "b": b }),
        "srvcs-multiply",
    )
    .await
    {
        Ok(body) => body,
        Err(resp) => return resp,
    };
    let result = match multiply_body.get("result").and_then(Value::as_i64) {
        Some(r) => r,
        None => return malformed("srvcs-multiply"),
    };

    ok(a, b, result)
}

#[derive(OpenApi)]
#[openapi(
    paths(index, evaluate),
    components(schemas(Info, EvalRequest, LcmResponse))
)]
pub struct ApiDoc;

/// Serve OpenAPI document
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_documents_routes() {
        let doc = ApiDoc::openapi();
        let root = doc.paths.paths.get("/").expect("path / present");
        assert!(root.get.is_some());
        assert!(root.post.is_some());
    }

    #[tokio::test]
    async fn index_reports_all_dependencies() {
        let Json(info) = index().await;
        assert_eq!(info.service, "srvcs-lcm");
        assert_eq!(info.concern, "number theory: least common multiple");
        assert_eq!(
            info.depends_on,
            vec!["srvcs-gcd", "srvcs-divide", "srvcs-multiply"]
        );
    }
}
