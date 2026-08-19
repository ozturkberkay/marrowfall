//! Meshy API client: image -> 3D -> rig -> animation. Every operation is
//! asynchronous; [`Client::run`] submits and polls so stages read as
//! straight-line code.
//!
//! The three endpoints put their result URL under different keys, so
//! [`Task::glb_url`] searches the known ones and the payload stays raw JSON, //! an unexpected shape then fails clearly rather than after the spend.

use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

use crate::spec::TextureResolution;
use serde_json::{Value, json};

const BASE: &str = "https://api.meshy.ai/openapi";

/// The skeleton this provider's auto-rigger produces. The adapter names the
/// core concept, never the other way round.
pub const RIGS: &str = crate::library::HUMANOID;
const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Generation occasionally takes minutes; this only guards against hangs.
const POLL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Per-request ceiling, so a hung connection cannot outlive the poll timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// How this API spells a texture size. The wire format is a string; a number
/// is rejected.
const fn texture_resolution(resolution: TextureResolution) -> &'static str {
    match resolution {
        TextureResolution::K2 => "2k",
        TextureResolution::K4 => "4k",
        TextureResolution::K8 => "8k",
    }
}

/// Terminal and in-flight states a Meshy task can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Succeeded,
    Failed,
    Canceled,
    /// Any status the API adds later. Treated as terminal-unknown rather than
    /// crashing a poll loop that has already been paid for.
    #[serde(other)]
    Unknown,
}

impl TaskStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }
}

/// A task's current state, plus the raw payload for fields that vary by
/// endpoint.
#[derive(Debug)]
pub struct Task {
    pub id: String,
    pub status: TaskStatus,
    pub progress: u32,
    pub credits: Option<u32>,
    pub payload: Value,
}

/// The subset of every task response that is shaped the same way.
#[derive(Debug, Deserialize)]
struct TaskEnvelope {
    #[serde(default)]
    id: String,
    status: TaskStatus,
    #[serde(default)]
    progress: u32,
    #[serde(default)]
    consumed_credits: Option<u32>,
    #[serde(default)]
    task_error: Option<TaskError>,
}

#[derive(Debug, Deserialize)]
struct TaskError {
    #[serde(default)]
    message: String,
}

impl Task {
    /// The finished GLB, wherever this endpoint chose to put it.
    pub fn glb_url(&self) -> Option<&str> {
        const KEYS: [&str; 3] = [
            "glb",                      // under model_urls
            "rigged_character_glb_url", // rigging
            "animation_glb_url",        // animation
        ];
        let roots = [
            self.payload.get("model_urls"),
            self.payload.get("result"),
            Some(&self.payload),
        ];
        roots
            .into_iter()
            .flatten()
            .flat_map(|root| KEYS.iter().filter_map(move |key| root.get(key)))
            .find_map(Value::as_str)
    }

    /// Preview renders Meshy generates for free, used for review without
    /// downloading the mesh.
    pub fn thumbnail_urls(&self) -> Vec<&str> {
        let roots = [self.payload.get("result"), Some(&self.payload)];
        for root in roots.into_iter().flatten() {
            if let Some(list) = root.get("thumbnail_urls").and_then(Value::as_array) {
                return list.iter().filter_map(Value::as_str).collect();
            }
            if let Some(one) = root.get("thumbnail_url").and_then(Value::as_str) {
                return vec![one];
            }
        }
        Vec::new()
    }
}

/// Which Meshy endpoint family a task belongs to. Status is polled from the
/// same path the task was created on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
pub enum Endpoint {
    MultiImageTo3d,
    Rigging,
    Animation,
}

impl Endpoint {
    pub const fn path(self) -> &'static str {
        match self {
            Self::MultiImageTo3d => "/v1/multi-image-to-3d",
            Self::Rigging => "/v1/rigging",
            Self::Animation => "/v1/animations",
        }
    }
}

#[derive(Debug)]
pub struct Client {
    http: reqwest::Client,
    api_key: String,
    /// API root. Overridable so the tests can serve the API locally; there is
    /// no other reason to change it.
    base: String,
}

impl Client {
    /// Reads the key from `MESHY_API_KEY`.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("MESHY_API_KEY").context(
            "MESHY_API_KEY is not set. Export it, or copy it from the meshy-mcp-server \
             entry in ~/.claude.json",
        )?;
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("building HTTP client")?;
        let base = std::env::var("MARROWFALL_MESHY_BASE_URL").unwrap_or_else(|_| BASE.to_owned());
        Ok(Self {
            http,
            api_key,
            base,
        })
    }

    /// Remaining credit balance.
    pub async fn balance(&self) -> Result<u32> {
        let response = self
            .http
            .get(format!("{}/v1/balance", self.base))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("requesting Meshy balance")?;
        let body: Value = self.decode(response, "balance").await?;
        Ok(body
            .get("balance")
            .and_then(Value::as_u64)
            .unwrap_or_default() as u32)
    }

    /// Submits a task and returns its id.
    pub async fn submit(&self, endpoint: Endpoint, body: Value) -> Result<String> {
        let response = self
            .http
            .post(format!("{}{}", self.base, endpoint.path()))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("submitting to {}", endpoint.path()))?;

        let payload: Value = self.decode(response, endpoint.path()).await?;
        payload
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .with_context(|| format!("no task id in response from {}", endpoint.path()))
    }

    /// Fetches a task's current state.
    pub async fn status(&self, endpoint: Endpoint, id: &str) -> Result<Task> {
        let response = self
            .http
            .get(format!("{}{}/{id}", self.base, endpoint.path()))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .with_context(|| format!("polling task {id}"))?;
        let payload: Value = self.decode(response, endpoint.path()).await?;
        let envelope: TaskEnvelope = serde_json::from_value(payload.clone())
            .with_context(|| format!("decoding task {id}"))?;

        if envelope.status.is_terminal() && envelope.status != TaskStatus::Succeeded {
            let reason = envelope
                .task_error
                .as_ref()
                .map(|error| error.message.as_str())
                .filter(|message| !message.is_empty())
                .unwrap_or("no reason given");
            bail!("Meshy task {id} ended as {:?}: {reason}", envelope.status);
        }

        Ok(Task {
            id: if envelope.id.is_empty() {
                id.to_owned()
            } else {
                envelope.id
            },
            status: envelope.status,
            progress: envelope.progress,
            credits: envelope.consumed_credits,
            payload,
        })
    }

    /// Submits a task and polls until it finishes.
    pub async fn run(
        &self,
        endpoint: Endpoint,
        body: Value,
        mut on_progress: impl FnMut(u32),
    ) -> Result<Task> {
        let id = self.submit(endpoint, body).await?;
        // Overridable so the tests can exercise the retry loop in milliseconds
        // rather than minutes; nothing else should set it.
        let interval = std::env::var("MARROWFALL_MESHY_POLL_MS")
            .ok()
            .and_then(|ms| ms.parse().ok())
            .map_or(POLL_INTERVAL, Duration::from_millis);
        let timeout = std::env::var("MARROWFALL_MESHY_TIMEOUT_MS")
            .ok()
            .and_then(|ms| ms.parse().ok())
            .map_or(POLL_TIMEOUT, Duration::from_millis);
        let deadline = tokio::time::Instant::now() + timeout;
        let mut last_progress = u32::MAX;

        loop {
            let task = self.status(endpoint, &id).await?;

            if task.progress != last_progress {
                last_progress = task.progress;
                on_progress(task.progress);
            }
            if task.status == TaskStatus::Succeeded {
                return Ok(task);
            }
            if task.status == TaskStatus::Unknown {
                bail!(
                    "Meshy task {id} reported an unrecognised status; \
                     check the Meshy dashboard"
                );
            }
            if tokio::time::Instant::now() >= deadline {
                bail!(
                    "Meshy task {id} still {:?} after {} minutes; check the Meshy dashboard",
                    task.status,
                    timeout.as_secs() / 60
                );
            }
            tokio::time::sleep(interval).await;
        }
    }

    /// Fetches a URL's bytes without writing them to disk.
    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        let bytes = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("fetching {url}"))?
            .error_for_status()
            .with_context(|| format!("fetching {url}"))?
            .bytes()
            .await?;
        Ok(bytes.to_vec())
    }

    /// Downloads a result file to disk.
    pub async fn download(&self, url: &str, dest: &std::path::Path) -> Result<()> {
        let bytes = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("downloading {url}"))?
            .error_for_status()
            .with_context(|| format!("downloading {url}"))?
            .bytes()
            .await?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(dest, &bytes).with_context(|| format!("writing {}", dest.display()))?;
        Ok(())
    }

    /// Decodes a JSON response, turning HTTP errors into messages that name
    /// the endpoint and quote the server's own explanation.
    async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
        what: &str,
    ) -> Result<T> {
        let status = response.status();
        let text = response
            .text()
            .await
            .with_context(|| format!("reading {what} response body"))?;
        if !status.is_success() {
            bail!("Meshy {what} returned {status}: {}", text.trim());
        }
        serde_json::from_str(&text)
            .with_context(|| format!("decoding {what} response: {}", truncate(&text, 300)))
    }
}

/// Body for a multi-view image-to-3D task. Remesh and texture are parameters
/// because the API performs both inline.
pub fn image_to_3d_body(
    image_data_uris: &[String],
    target_polycount: u32,
    quads: bool,
    pbr: bool,
    resolution: TextureResolution,
) -> Value {
    json!({
        "image_urls": image_data_uris,
        "should_remesh": true,
        "target_polycount": target_polycount,
        "topology": if quads { "quad" } else { "triangle" },
        "should_texture": true,
        "enable_pbr": pbr,
        "texture_resolution": texture_resolution(resolution),
        "ai_model": "meshy-7",
    })
}

/// Body for auto-rigging a generated mesh. No body-plan parameter: the
/// endpoint only supports bipedal humanoids.
pub fn rigging_body(model_task_id: &str, height_meters: f32) -> Value {
    json!({
        "input_task_id": model_task_id,
        "height_meters": height_meters,
    })
}

/// Body for attaching one library animation to a rigged character. Selected
/// by numeric id, not by name.
pub fn animation_body(rig_task_id: &str, action_id: u32) -> Value {
    json!({
        "rig_task_id": rig_task_id,
        "action_id": action_id,
    })
}

/// Inlines an image into a request body. The API documents data URIs as an
/// accepted form of `image_urls`, so concepts need no hosting.
pub fn to_data_uri(png: &[u8]) -> String {
    use base64::Engine as _;
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

pub fn truncate(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        // Slice on a char boundary: this runs while reporting another error,
        // and panicking here would hide it.
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_owned(),
    }
}
