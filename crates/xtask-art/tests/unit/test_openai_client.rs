//! The OpenAI image client, served by a local mock rather than the real API.

use base64::Engine as _;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::openai::Client;

use crate::support::{EnvGuard, a_png};

async fn client(server: &MockServer) -> Client {
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    Client::from_env().expect("client from env")
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[tokio::test]
async fn missing_api_key_names_the_variable() {
    let mut env = EnvGuard::new();
    env.remove("OPENAI_API_KEY");
    let error = Client::from_env().unwrap_err().to_string();
    assert!(error.contains("OPENAI_API_KEY"), "got: {error}");
}

#[tokio::test]
async fn generate_returns_the_decoded_image() {
    let server = MockServer::start().await;
    let png = a_png();
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": b64(&png)}]})),
        )
        .mount(&server)
        .await;

    let bytes = client(&server).await.generate("a survivor").await.unwrap();
    assert_eq!(bytes, png);
}

#[tokio::test]
async fn a_url_response_is_followed_rather_than_returned_as_text() {
    let server = MockServer::start().await;
    let png = a_png();
    Mock::given(method("GET"))
        .and(path("/hosted.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png.clone()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"url": format!("{}/hosted.png", server.uri())}]
        })))
        .mount(&server)
        .await;

    let bytes = client(&server).await.generate("a survivor").await.unwrap();
    assert_eq!(bytes, png);
}

#[tokio::test]
async fn edit_sends_the_reference_image_and_returns_the_result() {
    let server = MockServer::start().await;
    let png = a_png();
    Mock::given(method("POST"))
        .and(path("/images/edits"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": b64(&png)}]})),
        )
        .mount(&server)
        .await;

    let bytes = client(&server)
        .await
        .edit("same character, from behind", &png)
        .await
        .unwrap();
    assert_eq!(bytes, png);
}

#[tokio::test]
async fn an_http_error_quotes_the_status_and_the_servers_explanation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limit reached"))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .generate("x")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("429"), "status missing: {error}");
    assert!(error.contains("rate limit"), "reason missing: {error}");
}

#[tokio::test]
async fn an_empty_data_array_is_an_error_rather_than_an_empty_png() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .generate("x")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("no image"), "got: {error}");
}

#[tokio::test]
async fn a_non_json_body_is_reported_as_a_decode_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .generate("x")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("decoding"), "got: {error}");
}

#[tokio::test]
async fn malformed_base64_is_reported_rather_than_written_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": "!!!"}]})),
        )
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .generate("x")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("base64"), "got: {error}");
}
