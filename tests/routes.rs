use axum::body::Body;
use axum::extract::Json as AxumJson;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router as AxumRouter};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use srvcs_lcm::{api::Deps, health, router, telemetry};
use tower::ServiceExt;

const DEAD_URL: &str = "http://127.0.0.1:1";

/// Spawn a *computing* mock `srvcs-gcd`: reads `{"a": x, "b": y}` and returns
/// `{"result": gcd(x, y)}` — the real greatest common divisor (with the srvcs
/// convention `gcd(0, 0) == 0`). The lcm orchestration is genuinely driven by
/// this answer rather than a canned value.
async fn spawn_gcd() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|AxumJson(body): AxumJson<Value>| async move {
            let a = body.get("a").and_then(Value::as_i64).unwrap_or(0);
            let b = body.get("b").and_then(Value::as_i64).unwrap_or(0);
            Json(json!({ "result": gcd(a, b) }))
        }),
    );
    serve(app).await
}

/// Spawn a *computing* mock `srvcs-divide`: reads `{"a": x, "b": y}` and
/// returns `{"result": x / y}` (integer division), or `422` on divide-by-zero.
async fn spawn_divide() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|AxumJson(body): AxumJson<Value>| async move {
            let a = body.get("a").and_then(Value::as_i64).unwrap_or(0);
            let b = body.get("b").and_then(Value::as_i64).unwrap_or(1);
            if b == 0 {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({ "error": "divide by zero" })),
                );
            }
            (StatusCode::OK, Json(json!({ "result": a / b })))
        }),
    );
    serve(app).await
}

/// Spawn a *computing* mock `srvcs-multiply`: reads `{"a": x, "b": y}` and
/// returns `{"result": x * y}` — the real product.
async fn spawn_multiply() -> String {
    let app = AxumRouter::new().route(
        "/",
        post(|AxumJson(body): AxumJson<Value>| async move {
            let a = body.get("a").and_then(Value::as_i64).unwrap_or(0);
            let b = body.get("b").and_then(Value::as_i64).unwrap_or(0);
            Json(json!({ "result": a * b }))
        }),
    );
    serve(app).await
}

/// Spawn a mock returning a fixed status + body (used for error-path tests).
async fn spawn_fixed(status: StatusCode, body: Value) -> String {
    let app = AxumRouter::new().route(
        "/",
        post(move || {
            let body = body.clone();
            async move { (status, Json(body)) }
        }),
    );
    serve(app).await
}

async fn serve(app: AxumRouter) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// Real gcd used by the gcd mock so the orchestration is tested against true
/// answers (matches the srvcs convention `gcd(0, 0) == 0`).
fn gcd(a: i64, b: i64) -> i64 {
    let (mut x, mut y) = (a, b);
    while y != 0 {
        let r = x % y;
        x = y;
        y = r;
    }
    x
}

fn app(gcd_url: &str, divide_url: &str, multiply_url: &str) -> axum::Router {
    router(
        telemetry::metrics_handle_for_tests(),
        Deps {
            gcd_url: gcd_url.to_string(),
            divide_url: divide_url.to_string(),
            multiply_url: multiply_url.to_string(),
        },
    )
}

async fn lcm(
    gcd_url: &str,
    divide_url: &str,
    multiply_url: &str,
    a: i64,
    b: i64,
) -> (StatusCode, Value) {
    let res = app(gcd_url, divide_url, multiply_url)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "a": a, "b": b }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn status_of(uri: &str) -> StatusCode {
    app(DEAD_URL, DEAD_URL, DEAD_URL)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

// --- Standard endpoints. ---

#[tokio::test]
async fn healthz_ok() {
    assert_eq!(status_of("/healthz").await, StatusCode::OK);
}

#[tokio::test]
async fn readyz_reflects_state() {
    health::set_ready(true);
    assert_eq!(status_of("/readyz").await, StatusCode::OK);
}

#[tokio::test]
async fn metrics_ok() {
    assert_eq!(status_of("/metrics").await, StatusCode::OK);
}

#[tokio::test]
async fn openapi_ok() {
    assert_eq!(status_of("/openapi.json").await, StatusCode::OK);
}

#[tokio::test]
async fn generates_request_id_when_absent() {
    let res = app(DEAD_URL, DEAD_URL, DEAD_URL)
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        res.headers().contains_key("x-request-id"),
        "response must carry a generated x-request-id"
    );
}

#[tokio::test]
async fn index_reports_identity() {
    let res = app(DEAD_URL, DEAD_URL, DEAD_URL)
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["service"], "srvcs-lcm");
    assert_eq!(body["concern"], "number theory: least common multiple");
    assert_eq!(
        body["depends_on"],
        json!(["srvcs-gcd", "srvcs-divide", "srvcs-multiply"])
    );
}

// --- Correctness cases, against the computing mocks. ---

#[tokio::test]
async fn lcm_4_6_is_12() {
    let (g, d, m) = (
        spawn_gcd().await,
        spawn_divide().await,
        spawn_multiply().await,
    );
    let (status, body) = lcm(&g, &d, &m, 4, 6).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["a"], 4);
    assert_eq!(body["b"], 6);
    // gcd(4,6)=2; 4/2=2; 2*6=12
    assert_eq!(body["result"], 12);
}

#[tokio::test]
async fn lcm_21_6_is_42() {
    let (g, d, m) = (
        spawn_gcd().await,
        spawn_divide().await,
        spawn_multiply().await,
    );
    let (status, body) = lcm(&g, &d, &m, 21, 6).await;
    assert_eq!(status, StatusCode::OK);
    // gcd(21,6)=3; 21/3=7; 7*6=42
    assert_eq!(body["result"], 42);
}

#[tokio::test]
async fn lcm_coprime_3_5_is_15() {
    let (g, d, m) = (
        spawn_gcd().await,
        spawn_divide().await,
        spawn_multiply().await,
    );
    let (status, body) = lcm(&g, &d, &m, 3, 5).await;
    assert_eq!(status, StatusCode::OK);
    // gcd(3,5)=1; 3/1=3; 3*5=15
    assert_eq!(body["result"], 15);
}

#[tokio::test]
async fn lcm_equal_7_7_is_7() {
    let (g, d, m) = (
        spawn_gcd().await,
        spawn_divide().await,
        spawn_multiply().await,
    );
    let (status, body) = lcm(&g, &d, &m, 7, 7).await;
    assert_eq!(status, StatusCode::OK);
    // gcd(7,7)=7; 7/7=1; 1*7=7
    assert_eq!(body["result"], 7);
}

#[tokio::test]
async fn lcm_with_a_zero_runs_full_pipeline() {
    // gcd(0, 5) == 5 (not zero), so this exercises the full pipeline:
    // 0/5=0; 0*5=0.
    let (g, d, m) = (
        spawn_gcd().await,
        spawn_divide().await,
        spawn_multiply().await,
    );
    let (status, body) = lcm(&g, &d, &m, 0, 5).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], 0);
}

#[tokio::test]
async fn lcm_0_0_is_0_without_more_calls() {
    // gcd(0,0)==0 -> result is 0 and divide/multiply are never called: point
    // those at dead ports to prove no call is made.
    let g = spawn_gcd().await;
    let (status, body) = lcm(&g, DEAD_URL, DEAD_URL, 0, 0).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], 0);
}

// --- Error / degraded paths. ---

#[tokio::test]
async fn degrades_when_gcd_unreachable() {
    let (d, m) = (spawn_divide().await, spawn_multiply().await);
    let (status, body) = lcm(DEAD_URL, &d, &m, 4, 6).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-gcd");
}

#[tokio::test]
async fn degrades_when_divide_unreachable() {
    // gcd is reachable and non-zero, so the loop reaches the divide call.
    let (g, m) = (spawn_gcd().await, spawn_multiply().await);
    let (status, body) = lcm(&g, DEAD_URL, &m, 4, 6).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-divide");
}

#[tokio::test]
async fn degrades_when_multiply_unreachable() {
    // gcd + divide reachable, so the pipeline reaches the multiply call.
    let (g, d) = (spawn_gcd().await, spawn_divide().await);
    let (status, body) = lcm(&g, &d, DEAD_URL, 4, 6).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["dependency"], "srvcs-multiply");
}

#[tokio::test]
async fn forwards_422_from_gcd() {
    let (d, m) = (spawn_divide().await, spawn_multiply().await);
    let g = spawn_fixed(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "error": "value is not an integer" }),
    )
    .await;
    let (status, _) = lcm(&g, &d, &m, 4, 6).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn forwards_422_from_divide() {
    // gcd computes a real (non-zero) result so the pipeline reaches divide,
    // which rejects -> forward 422.
    let (g, m) = (spawn_gcd().await, spawn_multiply().await);
    let d = spawn_fixed(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "error": "bad operand" }),
    )
    .await;
    let (status, _) = lcm(&g, &d, &m, 4, 6).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn forwards_422_from_multiply() {
    let (g, d) = (spawn_gcd().await, spawn_divide().await);
    let m = spawn_fixed(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({ "error": "bad operand" }),
    )
    .await;
    let (status, _) = lcm(&g, &d, &m, 4, 6).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn malformed_gcd_result_is_500() {
    // gcd answers 200 but with no integer result -> contract violation -> 500.
    let (d, m) = (spawn_divide().await, spawn_multiply().await);
    let g = spawn_fixed(StatusCode::OK, json!({ "result": "not-a-number" })).await;
    let (status, body) = lcm(&g, &d, &m, 4, 6).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["dependency"], "srvcs-gcd");
}
