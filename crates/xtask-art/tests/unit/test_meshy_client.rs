//! The Meshy HTTP client, served by a local mock rather than the real API.

use serde_json::json;
use wiremock::matchers::{body_json_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xtask_art::meshy::{Client, Endpoint};

use crate::support::EnvGuard;

async fn client(server: &MockServer) -> Client {
    let mut env = EnvGuard::new();
    env.with_api(&server.uri());
    // The client copies the base at construction, so the guard can drop here.
    Client::from_env().expect("client from env")
}

#[tokio::test]
async fn missing_api_key_names_the_variable_and_where_to_find_it() {
    let mut env = EnvGuard::new();
    env.remove("MESHY_API_KEY");
    let error = Client::from_env().unwrap_err().to_string();
    assert!(error.contains("MESHY_API_KEY"), "got: {error}");
}

#[tokio::test]
async fn balance_reads_the_remaining_credits() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 412})))
        .mount(&server)
        .await;

    assert_eq!(client(&server).await.balance().await.unwrap(), 412);
}

#[tokio::test]
async fn a_balance_response_without_the_field_reads_as_zero() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&server)
        .await;

    assert_eq!(client(&server).await.balance().await.unwrap(), 0);
}

#[tokio::test]
async fn submit_sends_the_body_and_returns_the_task_id() {
    let server = MockServer::start().await;
    let body = json!({"input_task_id": "m1", "height_meters": 1.7});
    Mock::given(method("POST"))
        .and(path(Endpoint::Rigging.path()))
        .and(body_json_string(body.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "task-9"})))
        .mount(&server)
        .await;

    let id = client(&server)
        .await
        .submit(Endpoint::Rigging, body)
        .await
        .unwrap();
    assert_eq!(id, "task-9");
}

#[tokio::test]
async fn a_submit_response_without_a_task_id_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Rigging.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .submit(Endpoint::Rigging, json!({}))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("no task id"), "got: {error}");
}

#[tokio::test]
async fn an_http_error_quotes_the_status_and_the_servers_own_explanation() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(402).set_body_string("out of credits"))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .balance()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("402"), "status missing: {error}");
    assert!(error.contains("out of credits"), "reason missing: {error}");
}

#[tokio::test]
async fn a_non_json_success_body_reports_what_it_failed_to_decode() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;

    let error = format!("{:#}", client(&server).await.balance().await.unwrap_err());
    assert!(error.contains("maintenance"), "got: {error}");
}

#[tokio::test]
async fn status_reports_progress_while_a_task_is_running() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{}/t1", Endpoint::Rigging.path())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t1", "status": "IN_PROGRESS", "progress": 40
        })))
        .mount(&server)
        .await;

    let task = client(&server)
        .await
        .status(Endpoint::Rigging, "t1")
        .await
        .unwrap();
    assert_eq!(task.progress, 40);
    assert_eq!(task.id, "t1");
}

#[tokio::test]
async fn status_falls_back_to_the_requested_id_when_the_body_omits_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{}/t2", Endpoint::Rigging.path())))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"status": "IN_PROGRESS", "progress": 5})),
        )
        .mount(&server)
        .await;

    let task = client(&server)
        .await
        .status(Endpoint::Rigging, "t2")
        .await
        .unwrap();
    assert_eq!(
        task.id, "t2",
        "a task with no id must still be identifiable"
    );
}

#[tokio::test]
async fn a_failed_task_surfaces_the_providers_reason() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{}/t3", Endpoint::Rigging.path())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t3",
            "status": "FAILED",
            "task_error": {"message": "mesh exceeds 300k faces"}
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .status(Endpoint::Rigging, "t3")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("300k faces"), "got: {error}");
}

#[tokio::test]
async fn a_failed_task_with_no_message_still_reports_the_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("{}/t4", Endpoint::Rigging.path())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t4", "status": "FAILED", "task_error": {"message": ""}
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .status(Endpoint::Rigging, "t4")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("no reason given"), "got: {error}");
}

#[tokio::test]
async fn run_polls_until_the_task_succeeds_and_reports_progress_once_per_change() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::MultiImageTo3d.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "t5"})))
        .mount(&server)
        .await;
    // The same progress value twice: the callback must not fire for a repeat.
    Mock::given(method("GET"))
        .and(path(format!("{}/t5", Endpoint::MultiImageTo3d.path())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t5", "status": "SUCCEEDED", "progress": 100,
            "model_urls": {"glb": "https://example.test/m.glb"}
        })))
        .mount(&server)
        .await;

    let mut seen = Vec::new();
    let task = client(&server)
        .await
        .run(Endpoint::MultiImageTo3d, json!({}), |p| seen.push(p))
        .await
        .unwrap();
    assert_eq!(task.glb_url(), Some("https://example.test/m.glb"));
    assert_eq!(seen, vec![100]);
}

#[tokio::test]
async fn run_refuses_a_status_it_does_not_recognise() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Animation.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "t6"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{}/t6", Endpoint::Animation.path())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t6", "status": "REHYDRATING", "progress": 1
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .await
        .run(Endpoint::Animation, json!({}), |_| {})
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("unrecognised status"), "got: {error}");
}

#[tokio::test]
async fn fetch_returns_the_bytes_without_touching_disk() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/thumb.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 2, 3]))
        .mount(&server)
        .await;

    let bytes = client(&server)
        .await
        .fetch(&format!("{}/thumb.png", server.uri()))
        .await
        .unwrap();
    assert_eq!(bytes, vec![1, 2, 3]);
}

#[tokio::test]
async fn fetch_reports_the_url_it_could_not_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gone.png"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let url = format!("{}/gone.png", server.uri());
    let error = client(&server)
        .await
        .fetch(&url)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains(&url), "got: {error}");
}

#[tokio::test]
async fn download_creates_missing_parent_directories() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/a.glb"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"glTF".to_vec()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("deep/nested/a.glb");
    client(&server)
        .await
        .download(&format!("{}/a.glb", server.uri()), &dest)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"glTF");
}

#[tokio::test]
async fn a_failed_download_writes_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing.glb"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("missing.glb");
    let error = client(&server)
        .await
        .download(&format!("{}/missing.glb", server.uri()), &dest)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("downloading"), "got: {error}");
    assert!(
        !dest.exists(),
        "a failed download must not leave a stub file"
    );
}

#[tokio::test]
async fn run_keeps_polling_a_task_that_is_still_working() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Rigging.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "t7"})))
        .mount(&server)
        .await;
    // Two answers: in progress, then done. The second poll only happens if the
    // loop waits and asks again.
    Mock::given(method("GET"))
        .and(path(format!("{}/t7", Endpoint::Rigging.path())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t7", "status": "IN_PROGRESS", "progress": 30
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{}/t7", Endpoint::Rigging.path())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t7", "status": "SUCCEEDED", "progress": 100
        })))
        .mount(&server)
        .await;

    let mut env = EnvGuard::new();
    env.with_api(&server.uri())
        .set("MARROWFALL_MESHY_POLL_MS", "1");
    let client = Client::from_env().unwrap();

    let mut seen = Vec::new();
    client
        .run(Endpoint::Rigging, json!({}), |p| seen.push(p))
        .await
        .unwrap();
    assert_eq!(seen, vec![30, 100], "progress is reported once per change");
}

#[tokio::test]
async fn a_task_that_never_finishes_gives_up_and_says_where_to_look() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(Endpoint::Rigging.path()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": "t8"})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{}/t8", Endpoint::Rigging.path())))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "t8", "status": "IN_PROGRESS", "progress": 1
        })))
        .mount(&server)
        .await;

    let mut env = EnvGuard::new();
    env.with_api(&server.uri())
        .set("MARROWFALL_MESHY_POLL_MS", "1")
        .set("MARROWFALL_MESHY_TIMEOUT_MS", "5");
    let client = Client::from_env().unwrap();

    let error = client
        .run(Endpoint::Rigging, json!({}), |_| {})
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("minutes"), "got: {error}");
    assert!(error.contains("dashboard"), "got: {error}");
}
