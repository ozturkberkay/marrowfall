//! Mixamo API client: find a motion, ask for it, wait, download the FBX.
//!
//! Neither the API nor the FBX format is documented by its owner, so every
//! response is handled as raw JSON and an unexpected shape fails with the
//! endpoint named, the way [`crate::meshy`] already does. Only the export and
//! the poll that follows it need a credential; search and product do not.
//!
//! The clip that arrives is authored on Mixamo's own body, with Mixamo's own
//! bone names and rest pose. `retarget_animation.py` is what makes it ours.

use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};

use crate::chrome::Token;
use crate::meshy::truncate;

const BASE: &str = "https://www.mixamo.com/api/v1";

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

/// How long to wait out a rate limit that carries no usable `Retry-After`.
const DEFAULT_BACKOFF: Duration = Duration::from_secs(5);
/// Attempts per request before a rate limit is reported as a failure.
const RATE_LIMIT_ATTEMPTS: usize = 5;

/// Below this, a "download" is an error page or a cut-off body rather than a
/// clip. The smallest real Mixamo export is tens of kilobytes.
const MIN_FBX_BYTES: usize = 1024;

/// One motion in Mixamo's catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub id: String,
    pub name: String,
    /// The phrase the site's own search matches on, which is often not the
    /// name: "Backward Walk" is described so and named "Walking Backward".
    pub description: String,
}

/// What an export request needs: the provider's own opaque parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Motion {
    pub name: String,
    /// Read from the product call and echoed back unchanged, so settings such
    /// as `mirror` and `inplace` keep the provider's defaults.
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
    /// API root. Overridable so the tests can serve the API locally; there is
    /// no other reason to change it.
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
            base: std::env::var("MARROWFALL_MIXAMO_BASE_URL").unwrap_or_else(|_| BASE.to_owned()),
            // Overridable so the tests can exercise the poll loop in
            // milliseconds rather than minutes; nothing else should set these.
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

    /// Asks Mixamo to render one motion onto the stock body.
    pub async fn export(&self, motion: &Motion, token: &Token) -> Result<()> {
        let request = self
            .http
            .post(format!("{}/animations/export", self.base))
            .header("X-Requested-With", "XMLHttpRequest")
            .bearer_auth(token.expose_secret())
            .json(&export_body(motion));
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
                     check https://www.mixamo.com",
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
    pub async fn motion_fbx(&self, product_id: &str, token: &Token) -> Result<Vec<u8>> {
        let motion = self.product(product_id).await?;
        println!("  exporting {:?} from Mixamo…", motion.name);
        self.export(&motion, token).await?;
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

/// The catalogue entries in a search response.
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

/// Body for exporting one motion.
pub fn export_body(motion: &Motion) -> Value {
    json!({
        "character_id": CHARACTER_ID,
        "type": "Motion",
        "product_name": motion.name,
        "gms_hash": [motion.gms_hash],
        "preferences": {
            "format": "fbx7",
            // With its skin: an FBX carries its rest pose in the skin's bind
            // pose, and one exported without a mesh comes back posed instead,
            // which is the pose the whole retarget is measured against.
            "skin": "true",
            "fps": "30",
            // The bake resamples anyway, so this only bounds how smooth that
            // resample can be.
            "reducekf": "0",
        },
    })
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
        // Undocumented API: anything unrecognised is polled again, and the
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

/// How long to wait after a rate limit (RFC 6585 section 4).
///
/// Seconds only. RFC 9110 also allows an HTTP-date, which is rare enough that
/// the fixed backoff covers it.
pub fn retry_after(header: Option<&str>) -> Duration {
    header
        .and_then(|value| value.trim().parse().ok())
        .map_or(DEFAULT_BACKOFF, Duration::from_secs)
}

fn millis_from(variable: &str, fallback: Duration) -> Duration {
    std::env::var(variable)
        .ok()
        .and_then(|ms| ms.parse().ok())
        .map_or(fallback, Duration::from_millis)
}
