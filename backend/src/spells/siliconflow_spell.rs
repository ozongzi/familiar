use agentix::request::{Content, ImageContent, ImageData};
use agentix::tool;
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::sandbox::SandboxManager;

pub struct SiliconFlowSpell {
    pub api_key: String,
    pub http: Client,
    pub sandbox: Arc<SandboxManager>,
    pub user_id: Uuid,
    pub conversation_id: Uuid,
}

#[derive(Deserialize)]
struct SfImage {
    url: String,
}

#[derive(Deserialize)]
struct SfResponse {
    #[serde(default)]
    images: Vec<SfImage>,
}

fn ext_from_url(url: &str) -> &'static str {
    let path = url.split('?').next().unwrap_or(url);
    match path.rsplit('.').next() {
        Some("jpg") | Some("jpeg") => "jpg",
        Some("webp") => "webp",
        Some("gif") => "gif",
        _ => "png",
    }
}

#[tool]
impl Tool for SiliconFlowSpell {
    /// Generate an image using SiliconFlow's image generation API.
    /// ⚠️ SFW only — the API has content moderation and will reject explicit/NSFW prompts.
    /// Results are saved to the conversation sandbox and returned as viewable images.
    ///
    /// Available models:
    ///   Kwai-Kolors/Kolors            high quality general-purpose (recommended)
    ///   stabilityai/stable-diffusion-3-medium   SD3
    ///   stabilityai/stable-diffusion-xl-base-1.0
    ///
    /// image_size: image dimensions, e.g. "1024x1024" (default), "512x512", "768x1024", "1024x768"
    /// n: number of images to generate (default 1, max 4)
    /// seed: random seed for reproducibility (optional)
    /// negative_prompt: what to avoid in the image (optional)
    ///
    /// model: SiliconFlow model ID
    /// prompt: image description
    /// image_size: output dimensions (optional, default "1024x1024")
    /// n: number of images (optional, default 1)
    /// negative_prompt: negative prompt (optional)
    /// seed: random seed (optional)
    async fn generate_image(
        &self,
        model: String,
        prompt: String,
        image_size: Option<String>,
        n: Option<u32>,
        negative_prompt: Option<String>,
        seed: Option<i64>,
    ) -> Vec<Content> {
        let mut body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "n": n.unwrap_or(1),
            "image_size": image_size.as_deref().unwrap_or("1024x1024"),
        });

        if let Some(neg) = negative_prompt {
            body["negative_prompt"] = serde_json::Value::String(neg);
        }
        if let Some(s) = seed {
            body["seed"] = serde_json::Value::Number(s.into());
        }

        let resp = self
            .http
            .post("https://api.siliconflow.cn/v1/images/generations")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await;

        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                let status = r.status();
                let msg = r.text().await.unwrap_or_default();
                return vec![Content::text(format!("SiliconFlow error {status}: {msg}"))];
            }
            Err(e) => return vec![Content::text(format!("request error: {e}"))],
        };

        let sf_resp: SfResponse = match resp.json().await {
            Ok(r) => r,
            Err(e) => return vec![Content::text(format!("response parse error: {e}"))],
        };

        if sf_resp.images.is_empty() {
            return vec![Content::text("SiliconFlow returned no images")];
        }

        let conv_dir = self
            .sandbox
            .get_conversation_dir(self.user_id, self.conversation_id);
        let _ = tokio::fs::create_dir_all(&conv_dir).await;

        let mut results: Vec<Content> = Vec::new();

        for img in &sf_resp.images {
            let ext = ext_from_url(&img.url);
            let filename = format!("img-{}.{}", Uuid::new_v4(), ext);
            let path = conv_dir.join(&filename);

            let bytes = match self.http.get(&img.url).send().await {
                Ok(r) if r.status().is_success() => match r.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        results.push(Content::text(format!("download error: {e}")));
                        continue;
                    }
                },
                Ok(r) => {
                    results.push(Content::text(format!(
                        "image download failed: HTTP {}",
                        r.status()
                    )));
                    continue;
                }
                Err(e) => {
                    results.push(Content::text(format!("image download error: {e}")));
                    continue;
                }
            };

            if let Err(e) = tokio::fs::write(&path, &bytes).await {
                results.push(Content::text(format!("sandbox write error: {e}")));
                continue;
            }

            results.push(Content::Image(ImageContent {
                data: ImageData::Url(format!("__sandbox__:{}", filename)),
                mime_type: format!("image/{}", ext),
            }));
            results.push(Content::text(format!("/workspace/{}", filename)));
        }

        results
    }
}
