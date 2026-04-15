use agentix::request::{Content, ImageContent, ImageData};
use base64::Engine as _;
use agentix::tool;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::sandbox::SandboxManager;

pub struct GenerateImageSpell {
    pub siliconflow_api_key: Option<String>,
    pub fal_api_key: Option<String>,
    pub http: Client,
    pub sandbox: Arc<SandboxManager>,
    pub user_id: Uuid,
    pub conversation_id: Uuid,
}

#[derive(Deserialize)]
struct ImageUrl {
    url: String,
    #[serde(default)]
    content_type: Option<String>,
}

#[derive(Deserialize)]
struct ImageResponse {
    #[serde(default)]
    images: Vec<ImageUrl>,
}

fn ext_from_mime(mime: Option<&str>, url: &str) -> &'static str {
    if let Some(m) = mime {
        if m.contains("jpeg") || m.contains("jpg") { return "jpg"; }
        if m.contains("webp") { return "webp"; }
        if m.contains("gif") { return "gif"; }
    }
    let path = url.split('?').next().unwrap_or(url);
    match path.rsplit('.').next() {
        Some("jpg") | Some("jpeg") => "jpg",
        Some("webp") => "webp",
        Some("gif") => "gif",
        _ => "png",
    }
}

#[tool]
impl Tool for GenerateImageSpell {
    /// Generate images using SiliconFlow or fal.ai.
    ///
    /// SiliconFlow models (SFW only): Kwai-Kolors/Kolors (recommended), stabilityai/stable-diffusion-3-medium, stabilityai/stable-diffusion-xl-base-1.0
    /// fal.ai models (NSFW supported): fal-ai/flux-pro/v1.1-ultra, fal-ai/flux-pro/v1.1, fal-ai/flux-pro (use safety_tolerance), fal-ai/flux-realism, fal-ai/flux/dev, rundiffusion-fal/juggernaut-flux/pro (use enable_safety_checker)
    ///
    /// provider: "siliconflow" or "fal"
    /// model: model ID
    /// prompt: image description
    /// image_size: SiliconFlow uses "1024x1024" (default) / "512x512" / "768x1024" / "1024x768"; fal.ai uses "portrait_4_3" (default) / "landscape_4_3" / "square_hd" / "square"
    /// n: number of images, default 1 max 4
    /// negative_prompt: negative prompt, SiliconFlow only
    /// seed: random seed (optional)
    /// safety_tolerance: fal.ai flux-pro models only, "1" strictest to "6" most permissive, use "6" for NSFW
    /// enable_safety_checker: fal.ai non-pro models only, set false to allow NSFW output
    async fn generate_image(
        &self,
        provider: String,
        model: String,
        prompt: String,
        image_size: Option<String>,
        n: Option<u32>,
        negative_prompt: Option<String>,
        seed: Option<i64>,
        safety_tolerance: Option<String>,
        enable_safety_checker: Option<bool>,
    ) -> Vec<Content> {
        match provider.to_lowercase().as_str() {
            "siliconflow" => {
                let Some(ref api_key) = self.siliconflow_api_key else {
                    return vec![Content::text("SiliconFlow API key not configured")];
                };
                self.generate_siliconflow(
                    api_key, model, prompt, image_size, n, negative_prompt, seed,
                ).await
            }
            "fal" => {
                let Some(ref api_key) = self.fal_api_key else {
                    return vec![Content::text("fal.ai API key not configured")];
                };
                self.generate_fal(
                    api_key, model, prompt, image_size, n, seed,
                    safety_tolerance, enable_safety_checker,
                ).await
            }
            other => vec![Content::text(format!("Unknown provider '{other}'. Use 'siliconflow' or 'fal'."))],
        }
    }
}

impl GenerateImageSpell {
    async fn generate_siliconflow(
        &self,
        api_key: &str,
        model: String,
        prompt: String,
        image_size: Option<String>,
        n: Option<u32>,
        negative_prompt: Option<String>,
        seed: Option<i64>,
    ) -> Vec<Content> {
        let mut body = json!({
            "model": model,
            "prompt": prompt,
            "n": n.unwrap_or(1),
            "image_size": image_size.as_deref().unwrap_or("1024x1024"),
        });
        if let Some(neg) = negative_prompt {
            body["negative_prompt"] = json!(neg);
        }
        if let Some(s) = seed {
            body["seed"] = json!(s);
        }

        let resp = self.http
            .post("https://api.siliconflow.cn/v1/images/generations")
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let parsed: ImageResponse = match r.json().await {
                    Ok(v) => v,
                    Err(e) => return vec![Content::text(format!("response parse error: {e}"))],
                };
                self.download_and_save(parsed.images).await
            }
            Ok(r) => {
                let status = r.status();
                let msg = r.text().await.unwrap_or_default();
                vec![Content::text(format!("SiliconFlow error {status}: {msg}"))]
            }
            Err(e) => vec![Content::text(format!("request error: {e}"))],
        }
    }

    async fn generate_fal(
        &self,
        api_key: &str,
        model: String,
        prompt: String,
        image_size: Option<String>,
        n: Option<u32>,
        seed: Option<i64>,
        safety_tolerance: Option<String>,
        enable_safety_checker: Option<bool>,
    ) -> Vec<Content> {
        let mut body = json!({
            "prompt": prompt,
            "num_images": n.unwrap_or(1),
            "image_size": image_size.as_deref().unwrap_or("portrait_4_3"),
        });
        if let Some(s) = seed { body["seed"] = json!(s); }
        if let Some(st) = safety_tolerance { body["safety_tolerance"] = json!(st); }
        if let Some(sc) = enable_safety_checker { body["enable_safety_checker"] = json!(sc); }

        let url = format!("https://fal.run/{model}");
        let resp = self.http
            .post(&url)
            .header("Authorization", format!("Key {api_key}"))
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let parsed: ImageResponse = match r.json().await {
                    Ok(v) => v,
                    Err(e) => return vec![Content::text(format!("response parse error: {e}"))],
                };
                self.download_and_save(parsed.images).await
            }
            Ok(r) => {
                let status = r.status();
                let msg = r.text().await.unwrap_or_default();
                vec![Content::text(format!("fal.ai error {status}: {msg}"))]
            }
            Err(e) => vec![Content::text(format!("request error: {e}"))],
        }
    }

    async fn download_and_save(&self, images: Vec<ImageUrl>) -> Vec<Content> {
        if images.is_empty() {
            return vec![Content::text("No images returned")];
        }

        let conv_dir = self.sandbox
            .get_conversation_dir(self.user_id, self.conversation_id)
            .join("public");
        let _ = tokio::fs::create_dir_all(&conv_dir).await;

        let mut results: Vec<Content> = Vec::new();

        for img in &images {
            let ext = ext_from_mime(img.content_type.as_deref(), &img.url);

            let bytes = match self.http.get(&img.url).send().await {
                Ok(r) if r.status().is_success() => match r.bytes().await {
                    Ok(b) => b,
                    Err(e) => { results.push(Content::text(format!("download error: {e}"))); continue; }
                },
                Ok(r) => { results.push(Content::text(format!("image download failed: HTTP {}", r.status()))); continue; }
                Err(e) => { results.push(Content::text(format!("image download error: {e}"))); continue; }
            };

            // Use MD5 of bytes as filename — db.append will compute the same hash
            // when converting base64 → __sandbox__: URL, so paths stay consistent.
            let hash = format!("{:x}", md5::compute(&bytes));
            let filename = format!("img-{}.{}", hash, ext);
            let path = conv_dir.join(&filename);

            if let Err(e) = tokio::fs::write(&path, &bytes).await {
                results.push(Content::text(format!("sandbox write error: {e}")));
                continue;
            }

            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let mime = format!("image/{}", ext);
            results.push(Content::Image(ImageContent {
                data: ImageData::Base64(b64),
                mime_type: mime,
            }));
            results.push(Content::text(format!("/workspace/public/{}", filename)));
        }

        results
    }
}
