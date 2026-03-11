//! Thin client for the OpenRouter embeddings endpoint (or any OpenAI-compatible provider).

use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct EmbeddingClient {
    client: Client,
    token: String,
    api_base: String,
    model: String,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedData>,
}

#[derive(Deserialize)]
struct EmbedData {
    embedding: Vec<f32>,
}

impl EmbeddingClient {
    pub fn new(
        token: impl Into<String>,
        api_base: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            token: token.into(),
            api_base: api_base.into(),
            model: model.into(),
        }
    }

    /// Embed a single string. Returns a vector of the model's native dimensionality.
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.api_base.trim_end_matches('/'));

        let resp: EmbedResponse = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&EmbedRequest {
                model: &self.model,
                input: text,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        resp.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .ok_or_else(|| anyhow::anyhow!("empty embedding response"))
    }
}
