//! OpenAI Images client, used only to generate concept references.
//!
//! Two calls are involved per character: one text-to-image for the front view,
//! then image edits for every other view with the front attached as reference.
//! Deriving the other views from the front is what keeps them the *same*
//! character, a fresh prompt would produce a different one each time.

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use serde_json::Value;

use crate::spec::View;

const BASE: &str = "https://api.openai.com/v1";
const MODEL: &str = "gpt-image-2";
/// Portrait, so a standing figure fills the frame.
const SIZE: &str = "1024x1536";

#[derive(Debug)]
pub struct Client {
    http: reqwest::Client,
    api_key: String,
    /// API root. Overridable so the tests can serve the API locally; there is
    /// no other reason to change it.
    base: String,
}

impl Client {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is not set")?;
        let base = std::env::var("MARROWFALL_OPENAI_BASE_URL").unwrap_or_else(|_| BASE.to_owned());
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            base,
        })
    }

    /// Generates the first view from a prompt alone.
    pub async fn generate(&self, prompt: &str) -> Result<Vec<u8>> {
        let response = self
            .http
            .post(format!("{}/images/generations", self.base))
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "model": MODEL,
                "prompt": prompt,
                "size": SIZE,
                "quality": "high",
                "n": 1,
            }))
            .send()
            .await
            .context("calling OpenAI image generation")?;
        Self::decode_image(response).await
    }

    /// Generates a further view, conditioned on an already-generated one so
    /// the result is recognisably the same character.
    pub async fn edit(&self, prompt: &str, reference_png: &[u8]) -> Result<Vec<u8>> {
        let part = reqwest::multipart::Part::bytes(reference_png.to_vec())
            .file_name("reference.png")
            .mime_str("image/png")?;
        let form = reqwest::multipart::Form::new()
            .text("model", MODEL)
            .text("prompt", prompt.to_owned())
            .text("size", SIZE)
            .text("quality", "high")
            .text("n", "1")
            .part("image[]", part);

        let response = self
            .http
            .post(format!("{}/images/edits", self.base))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .context("calling OpenAI image edit")?;
        Self::decode_image(response).await
    }

    /// Pulls the first image out of a response, accepting either the
    /// base64 payload or a URL.
    async fn decode_image(response: reqwest::Response) -> Result<Vec<u8>> {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!("OpenAI images returned {status}: {}", text.trim());
        }

        let body: Value = serde_json::from_str(&text).context("decoding OpenAI response")?;
        let first = body
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .context("OpenAI response contained no image")?;

        if let Some(b64) = first.get("b64_json").and_then(Value::as_str) {
            return base64::engine::general_purpose::STANDARD
                .decode(b64)
                .context("decoding base64 image");
        }
        if let Some(url) = first.get("url").and_then(Value::as_str) {
            let bytes = reqwest::get(url)
                .await
                .with_context(|| format!("fetching generated image {url}"))?
                .bytes()
                .await?;
            return Ok(bytes.to_vec());
        }
        bail!("OpenAI response had neither b64_json nor url")
    }
}

/// Where the camera sits for a view, phrased for this API. Exhaustive, so a
/// new view cannot reach the prompt without a camera clause.
const fn camera(view: View) -> &'static str {
    // Each profile says which way the face points *in the frame*. Asking for
    // "the character's right side" is ambiguous about whose right, and the
    // model then returns the same profile for both.
    match view {
        View::Front => "straight-on FRONT of the character, eye level",
        View::Back => "directly BEHIND the character (rotated 180 degrees), eye level",
        View::Left => {
            "the character's LEFT side, a 90-degree profile, eye level. He faces the \
             RIGHT edge of the frame: his nose, chest and toes all point RIGHT, and the \
             back of his head and heels are on the LEFT"
        }
        View::Right => {
            "the character's RIGHT side, a 90-degree profile, eye level. He faces the \
             LEFT edge of the frame: his nose, chest and toes all point LEFT, and the \
             back of his head and heels are on the RIGHT. This is the mirror image of \
             the left-side view, not a repeat of it"
        }
    }
}

/// Style, camera and framing rules shared by every view. These are
/// reconstruction references, not sprites: the isometric angle comes later,
/// from the Blender camera.
fn shared_rules(pose: &str) -> String {
    format!(
        "Full-body character reference for 3D reconstruction, in the style of a grim \
         dark-fantasy game: HD painterly realism, muted and desaturated colours. \
         Strictly orthographic with zero perspective or lens distortion. \
         POSE: {pose} \
         LIGHTING: even, flat, neutral light with minimal soft shadows, this is a \
         reconstruction reference, so no dramatic or coloured lighting. \
         FRAMING: the whole body inside the frame, centred, nothing cropped; head near \
         the top edge and feet near the bottom. \
         BACKGROUND: a completely flat, uniform, neutral grey (#8A8A8A) fill, no \
         ground, no cast shadow, no gradient, no props. The reconstructor segments \
         the subject from this backdrop, so it must stay perfectly even. \
         EXCLUDE: text, watermark, logo, UI, border, pedestal, scenery, second character, \
         perspective distortion, blur, depth of field, high-angle or isometric camera."
    )
}

/// Prompt for the first (front) view.
pub fn front_prompt(description: &str, pose: &str) -> String {
    format!(
        "{}\nCAMERA: {}.\nCHARACTER: {description}",
        shared_rules(pose),
        camera(View::Front)
    )
}

/// Prompt for a view derived from the front reference.
pub fn view_prompt(view: View, description: &str, pose: &str) -> String {
    format!(
        "Using the attached image as reference, render the SAME character from {}. \
         Keep the body, proportions, markings, colours and clothing identical to the \
         reference, at the same scale and vertical centring.\n{}\nCHARACTER: {description}",
        camera(view),
        shared_rules(pose)
    )
}
