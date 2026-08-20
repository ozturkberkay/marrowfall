//! The Mixamo HTTP client, served by a local mock rather than the real API.

use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::chrome::Token;
use xtask_art::mixamo::{CHARACTER_ID, Client};

use crate::support::EnvGuard;

/// An FBX-shaped body, long enough to pass the size check.
fn an_fbx() -> Vec<u8> {
    let mut fbx = b"Kaydara FBX Binary  \x00".to_vec();
    fbx.resize(60_000, 0x42);
    fbx
}

fn a_token() -> Token {
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-bearer-token");
    xtask_art::chrome::mixamo_token()
        .expect("token from the environment")
        .expect("a token")
}

async fn client(server: &MockServer) -> Client {
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_BASE_URL", &server.uri())
        .set("MARROWFALL_MIXAMO_POLL_MS", "1")
        .set("MARROWFALL_MIXAMO_TIMEOUT_MS", "500");
    // The client copies the settings at construction, so the guard can drop.
    Client::new().expect("client")
}

/// The catalogue and product calls, which need no credential at all.
async fn mount_product(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/products/c9ccc468"))
        .and(query_param("similar", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": "Walking Backward",
            "details": {"gms_hash": {"model-id": 123_530_901, "mirror": false}},
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn search_lists_what_the_catalogue_returned() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/products"))
        .and(query_param("type", "Motion"))
        .and(query_param("query", "Backward Walk"))
        .and(header("x-api-key", "mixamo2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"id": "c9ccc468", "name": "Walking Backward", "description": ""}]
        })))
        .mount(&server)
        .await;

    let found = client(&server).await.search("Backward Walk").await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "c9ccc468");
}

#[tokio::test]
async fn the_product_call_extracts_the_opaque_export_parameters() {
    let server = MockServer::start().await;
    mount_product(&server).await;

    let motion = client(&server).await.product("c9ccc468").await.unwrap();
    assert_eq!(motion.name, "Walking Backward");
    assert_eq!(motion.gms_hash["model-id"], 123_530_901);
}

#[tokio::test]
async fn a_product_without_export_parameters_fails_naming_the_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/products/c9ccc468"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"name": "x"})))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .product("c9ccc468")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("gms_hash"), "got: {error}");
}

#[tokio::test]
async fn an_html_body_fails_with_the_endpoint_named_and_the_body_quoted() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/products/c9ccc468"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;

    let error = format!(
        "{:#}",
        client(&server).await.product("c9ccc468").await.unwrap_err()
    );
    assert!(error.contains("/products/"), "names the endpoint: {error}");
    assert!(error.contains("maintenance"), "quotes the server: {error}");
}

/// The whole export: request it, poll until it is rendered, download it.
#[tokio::test]
async fn a_motion_is_exported_polled_and_downloaded() {
    let server = MockServer::start().await;
    mount_product(&server).await;
    Mock::given(method("POST"))
        .and(path("/animations/export"))
        .and(header("authorization", "Bearer a-bearer-token"))
        .and(header("x-requested-with", "XMLHttpRequest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/characters/{CHARACTER_ID}/monitor")))
        .and(header("authorization", "Bearer a-bearer-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "processing"})))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/characters/{CHARACTER_ID}/monitor")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "completed",
            "job_result": format!("{}/files/motion.fbx", server.uri()),
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/motion.fbx"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(an_fbx()))
        .mount(&server)
        .await;

    let bytes = client(&server)
        .await
        .motion_fbx("c9ccc468", &a_token())
        .await
        .unwrap();
    assert_eq!(bytes, an_fbx());
}

#[tokio::test]
async fn a_stale_session_says_so_instead_of_repeating_the_status_code() {
    let server = MockServer::start().await;
    mount_product(&server).await;
    Mock::given(method("POST"))
        .and(path("/animations/export"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .motion_fbx("c9ccc468", &a_token())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("log in"), "says what to do: {error}");
    assert!(
        !error.contains("a-bearer-token"),
        "must never quote the credential: {error}"
    );
}

#[tokio::test]
async fn a_rate_limited_request_backs_off_and_tries_again() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/products/c9ccc468"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("slow down"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount_product(&server).await;

    let motion = client(&server).await.product("c9ccc468").await.unwrap();
    assert_eq!(motion.name, "Walking Backward");
}

#[tokio::test]
async fn a_server_that_only_rate_limits_gives_up_and_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/products/c9ccc468"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .product("c9ccc468")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("rate limit"), "got: {error}");
}

#[tokio::test]
async fn an_export_that_never_finishes_gives_up_and_says_where_to_look() {
    let server = MockServer::start().await;
    mount_product(&server).await;
    Mock::given(method("POST"))
        .and(path("/animations/export"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/characters/{CHARACTER_ID}/monitor")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "processing"})))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .motion_fbx("c9ccc468", &a_token())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("mixamo.com"), "says where to look: {error}");
}

#[tokio::test]
async fn a_download_that_is_not_a_clip_fails_before_anything_is_written() {
    let server = MockServer::start().await;
    mount_product(&server).await;
    Mock::given(method("POST"))
        .and(path("/animations/export"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/characters/{CHARACTER_ID}/monitor")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "completed",
            "job_result": format!("{}/files/motion.fbx", server.uri()),
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/motion.fbx"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>gone</html>"))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .motion_fbx("c9ccc468", &a_token())
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("not an FBX"), "got: {error}");
}

#[tokio::test]
async fn a_download_url_that_fails_names_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/files/missing.fbx"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let url = format!("{}/files/missing.fbx", server.uri());
    let error = format!(
        "{:#}",
        client(&server).await.download(&url).await.unwrap_err()
    );
    assert!(error.contains("missing.fbx"), "got: {error}");
}
