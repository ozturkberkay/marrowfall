//! Mixamo API client: find a motion, ask for it, wait, download the FBX.
//!
//! Neither the API nor the FBX format is documented by its owner, so every
//! response is handled as raw JSON and an unexpected shape fails with the
//! endpoint named. Only the export and the poll that follows it need a
//! credential; search and product do not.
//!
//! The clip that arrives is authored on Mixamo's own body, with Mixamo's own
//! bone names and rest pose. `retarget_animation.py` is what makes it ours.

use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};

use super::SITE_URL;
use super::session::Token;
use crate::http::{base_url, millis_from, retry_after, truncate};

/// Mixamo's own web client sends this on every call, and so must we.
const API_KEY: &str = "mixamo2";

/// The stock body Mixamo renders the motion onto. Y Bot is its neutral
/// default; the retarget discards the body, so any humanoid would do, and
/// Mixamo has no non-humanoid motion for a second one to matter.
pub const CHARACTER_ID: &str = "4f5d21e1-4ccc-41f1-b35b-fb2547bd8493";

/// Rendering a clip takes seconds, not minutes.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Attempts per request before a rate limit is reported as a failure.
const RATE_LIMIT_ATTEMPTS: usize = 5;

/// Below this, a "download" is an error page or a cut-off body rather than a
/// clip. The smallest real Mixamo export is tens of kilobytes.
const MIN_FBX_BYTES: usize = 1024;

/// One motion in Mixamo's catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: String,
    pub name: String,
    /// The phrase the site's own search matches on, which is often not the
    /// name: "Backward Walk" is described so and named "Walking Backward".
    pub description: String,
}

/// The `gms_hash` key that decides whether the clip travels.
///
/// Every other export setting is echoed back untouched. This one is asked for
/// in writing, because decision 12 fetches every source traveling. The name
/// is unconfirmed against a live response, so `source.traveling` is the guard
/// that would catch an in-place export either way.
const IN_PLACE: &str = "inplace";

/// What an export request needs: the provider's own opaque parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Motion {
    pub name: String,
    /// Read from the product call and echoed back with the in-place flag
    /// set, so settings such as `mirror` keep the provider's defaults.
    pub gms_hash: Value,
}

/// Where an export has got to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// Still rendering; poll again.
    Working,
    /// Finished, with the file to download.
    Ready(String),
}

#[derive(Debug)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    poll_interval: Duration,
    poll_timeout: Duration,
}

impl Client {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("building HTTP client")?;
        Ok(Self {
            http,
            base: base_url("MARROWFALL_MIXAMO_BASE_URL", &format!("{SITE_URL}/api/v1")),
            poll_interval: millis_from("MARROWFALL_MIXAMO_POLL_MS", POLL_INTERVAL),
            poll_timeout: millis_from("MARROWFALL_MIXAMO_TIMEOUT_MS", POLL_TIMEOUT),
        })
    }

    /// Motions matching a phrase. How a product id is found before it is
    /// written into the animation library.
    pub async fn search(&self, query: &str) -> Result<Vec<Product>> {
        let request = self.http.get(format!("{}/products", self.base)).query(&[
            ("page", "1"),
            ("limit", "96"),
            ("order", ""),
            ("type", "Motion"),
            ("query", query),
        ]);
        let payload = self.get_json(request, "/products").await?;
        Ok(products_in(&payload))
    }

    /// The export parameters for one product.
    pub async fn product(&self, product_id: &str) -> Result<Motion> {
        let what = format!("/products/{product_id}");
        let request = self
            .http
            .get(format!("{}{what}", self.base))
            .query(&[("similar", "0"), ("character_id", CHARACTER_ID)]);
        let payload = self.get_json(request, &what).await?;

        let gms_hash = payload
            .pointer("/details/gms_hash")
            .with_context(|| format!("no details.gms_hash in the response from {what}"))?;
        Ok(Motion {
            name: payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(product_id)
                .to_owned(),
            gms_hash: gms_hash.clone(),
        })
    }

    /// Asks Mixamo to render one motion onto the stock body, at the rate the
    /// library declares for it.
    pub async fn export(&self, motion: &Motion, source_fps: u32, token: &Token) -> Result<()> {
        let request = self
            .http
            .post(format!("{}/animations/export", self.base))
            .header("X-Requested-With", "XMLHttpRequest")
            .bearer_auth(token.expose_secret())
            .json(&export_body(motion, source_fps)?);
        self.get_json(request, "/animations/export").await?;
        Ok(())
    }

    /// Polls until the export is rendered, and returns the file to download.
    pub async fn monitor(&self, token: &Token) -> Result<String> {
        let what = format!("/characters/{CHARACTER_ID}/monitor");
        let deadline = tokio::time::Instant::now() + self.poll_timeout;
        loop {
            let request = self
                .http
                .get(format!("{}{what}", self.base))
                .bearer_auth(token.expose_secret());
            if let Progress::Ready(url) = progress_in(&self.get_json(request, &what).await?)? {
                return Ok(url);
            }
            if tokio::time::Instant::now() >= deadline {
                bail!(
                    "Mixamo has not finished this export after {} minute(s); \
                     check {SITE_URL}",
                    self.poll_timeout.as_secs().div_ceil(60)
                );
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    /// Fetches a rendered clip, and proves it is one.
    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .send(self.http.get(url), url)
            .await?
            .error_for_status()
            .with_context(|| format!("downloading {url}"))?;
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("reading {url}"))?;
        check_fbx(&bytes)?;
        Ok(bytes.to_vec())
    }

    /// The whole export: look the product up, ask for it, wait, download it.
    pub async fn motion_fbx(
        &self,
        product_id: &str,
        source_fps: u32,
        token: &Token,
    ) -> Result<Vec<u8>> {
        let motion = self.product(product_id).await?;
        println!("  exporting {:?} from Mixamo…", motion.name);
        self.export(&motion, source_fps, token).await?;
        let url = self.monitor(token).await?;
        self.download(&url).await
    }

    /// Sends a request, waiting out a rate limit rather than failing on it.
    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        what: &str,
    ) -> Result<reqwest::Response> {
        for _ in 0..RATE_LIMIT_ATTEMPTS {
            let attempt = request
                .try_clone()
                .with_context(|| format!("retrying {what}"))?;
            let response = attempt
                .header("X-Api-Key", API_KEY)
                .header("Accept", "application/json")
                .send()
                .await
                .with_context(|| format!("requesting {what}"))?;
            if response.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Ok(response);
            }
            let header = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok());
            tokio::time::sleep(retry_after(header)).await;
        }
        bail!("Mixamo kept rate limiting {what}; wait a few minutes and try again")
    }

    /// Sends a request and decodes the JSON it answers with, turning HTTP
    /// errors into messages that name the endpoint and quote the server.
    async fn get_json(&self, request: reqwest::RequestBuilder, what: &str) -> Result<Value> {
        let response = self.send(request, what).await?;
        let status = response.status();
        let text = response
            .text()
            .await
            .with_context(|| format!("reading the {what} response"))?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            // Never quote the request here: the credential is in it.
            bail!("Mixamo refused {what}: the session has expired, log in again in Chrome");
        }
        anyhow::ensure!(
            status.is_success(),
            "Mixamo {what} returned {status}: {}",
            truncate(text.trim(), 300)
        );
        serde_json::from_str(&text)
            .with_context(|| format!("decoding the {what} response: {}", truncate(&text, 300)))
    }
}

/// The catalog entries in a search response.
pub fn products_in(payload: &Value) -> Vec<Product> {
    let text = |product: &Value, key| {
        product
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    payload
        .get("results")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|product| {
            Some(Product {
                id: product.get("id").and_then(Value::as_str)?.to_owned(),
                name: text(product, "name"),
                description: text(product, "description"),
            })
        })
        .collect()
}

/// Body for exporting one motion, at the rate the library declares.
///
/// `source_fps` is the library's own field rather than a literal, so the rate
/// the clip is rendered at and the rate the retarget reads it at are one
/// number. Two would drift, and a 30 fps clip read on a 24 fps grid lands at
/// frames 0.8 to 16.8.
pub fn export_body(motion: &Motion, source_fps: u32) -> Result<Value> {
    Ok(json!({
        "character_id": CHARACTER_ID,
        "type": "Motion",
        "product_name": motion.name,
        "gms_hash": [traveling(flatten_params(&motion.gms_hash))?],
        "preferences": {
            "format": "fbx7",
            // With its skin: an FBX carries its rest pose in the skin's bind
            // pose, and one exported without a mesh comes back posed instead,
            // which is the pose the whole retarget is measured against.
            "skin": "true",
            "fps": source_fps.to_string(),
            // The bake resamples anyway, so this only bounds how smooth that
            // resample can be.
            "reducekf": "0",
        },
    }))
}

/// Asks for the clip with its root motion, in writing.
///
/// The field is opaque, so nothing before this says it has keys to set.
fn traveling(mut gms_hash: Value) -> Result<Value> {
    let Some(keys) = gms_hash.as_object_mut() else {
        bail!(
            "/animations/export needs a gms_hash with keys to set {IN_PLACE:?} on, \
             and the product call returned {}",
            shape_of(&gms_hash)
        );
    };
    keys.insert(IN_PLACE.to_owned(), Value::Bool(false));
    Ok(gms_hash)
}

/// What a JSON value is, for an error a human reads.
fn shape_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Collapses `params` from the pairs the product call returns into the
/// comma-joined values the export call wants.
///
/// The two endpoints disagree about the shape of the same field: the product
/// gives `[["Overdrive", 0], ..]` and the export wants `"0,.."`. Sending the
/// pairs back is accepted and then fails the render with "The job failed".
fn flatten_params(gms_hash: &Value) -> Value {
    let mut hash = gms_hash.clone();
    let Some(pairs) = hash.get("params").and_then(Value::as_array) else {
        return hash;
    };
    let values: Vec<String> = pairs
        .iter()
        .map(|pair| match pair.get(1) {
            Some(Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        })
        .collect();
    hash["params"] = Value::String(values.join(","));
    hash
}

/// What the monitor endpoint says about an export in flight.
pub fn progress_in(payload: &Value) -> Result<Progress> {
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if status == "failed" {
        let reason = payload
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("no reason given");
        bail!("Mixamo could not render this motion: {reason}");
    }
    if status != "completed" {
        // Undocumented API: anything unrecognized is polled again, and the
        // deadline is what stops a run that never finishes.
        return Ok(Progress::Working);
    }
    let url = payload
        .get("job_result")
        .and_then(Value::as_str)
        .context("Mixamo reported the export as done but named no file to download")?;
    Ok(Progress::Ready(url.to_owned()))
}

/// Refuses bytes that are not a clip.
///
/// The failure that actually happens is an HTML error page or a cut-off body
/// returned with a success status, which would otherwise reach Blender.
pub fn check_fbx(bytes: &[u8]) -> Result<()> {
    anyhow::ensure!(
        bytes.starts_with(b"Kaydara FBX Binary"),
        "not an FBX: {}",
        truncate(&String::from_utf8_lossy(&bytes[..bytes.len().min(80)]), 80)
    );
    anyhow::ensure!(
        bytes.len() > MIN_FBX_BYTES,
        "the FBX is truncated, {} bytes",
        bytes.len()
    );
    Ok(())
}
