use crate::error::GatewayError;
use crate::spelling::CustomSpelling;

const BASE_URL: &str = "https://api.assemblyai.com/v2";

/// AssemblyAI REST API client.
///
/// Wraps a `reqwest::Client` and an API key. The HTTP client is shared with the
/// rest of the gateway (via `AppState`) so connection pools are reused.
pub struct Client {
    api_key: String,
    http: reqwest::Client,
}

#[derive(serde::Deserialize)]
struct UploadResponse {
    upload_url: String,
}

/// Request body for POST /v2/transcript.
#[derive(serde::Serialize)]
struct TranscriptRequest<'a> {
    audio_url: &'a str,
    punctuate: bool,
    format_text: bool,
    language_detection: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    custom_spelling: Vec<AssemblyAISpelling>,
}

/// AssemblyAI's custom_spelling format: each entry maps multiple "from" variants
/// to a single "to" word. We flatten our internal `CustomSpelling` (one from -> one to)
/// into this format at serialization time.
#[derive(serde::Serialize)]
struct AssemblyAISpelling {
    from: Vec<String>,
    to: String,
}

/// Response from GET /v2/transcript/{id}.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TranscriptResponse {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub audio_duration: Option<f64>,
}

impl Client {
    /// Create a new AssemblyAI client.
    ///
    /// The `http` client should have a generous timeout (300s) since upload and
    /// polling can take a while. The caller typically builds a dedicated client
    /// or reuses `AppState::http_client` with an appropriate timeout.
    pub fn new(api_key: String, http: reqwest::Client) -> Self {
        Self { api_key, http }
    }

    /// Upload raw audio bytes to AssemblyAI.
    ///
    /// Returns the temporary `upload_url` that can be passed to `create_transcript`.
    pub async fn upload(&self, audio_data: &[u8]) -> Result<String, GatewayError> {
        let url = format!("{BASE_URL}/upload");

        let resp = self
            .http
            .post(&url)
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/octet-stream")
            .body(audio_data.to_vec())
            .send()
            .await
            .map_err(|e| GatewayError::AssemblyAI(format!("upload request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(GatewayError::AssemblyAI(format!(
                "upload failed with status {status}: {body}"
            )));
        }

        let upload_resp: UploadResponse = resp
            .json()
            .await
            .map_err(|e| GatewayError::AssemblyAI(format!("failed to parse upload response: {e}")))?;

        Ok(upload_resp.upload_url)
    }

    /// Create a transcript job on AssemblyAI.
    ///
    /// Returns the transcript ID for polling via `get_transcript`.
    pub async fn create_transcript(
        &self,
        audio_url: &str,
        custom_spelling: &[CustomSpelling],
    ) -> Result<String, GatewayError> {
        let url = format!("{BASE_URL}/transcript");

        // Convert our flat CustomSpelling list into AssemblyAI's grouped format.
        // Each unique "to" gets all its "from" variants grouped together.
        let aai_spelling = group_spelling(custom_spelling);

        let body = TranscriptRequest {
            audio_url,
            punctuate: true,
            format_text: true,
            language_detection: true,
            custom_spelling: aai_spelling,
        };

        let resp = self
            .http
            .post(&url)
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                GatewayError::AssemblyAI(format!("create transcript request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(GatewayError::AssemblyAI(format!(
                "transcript creation failed with status {status}: {body}"
            )));
        }

        let transcript_resp: TranscriptResponse = resp.json().await.map_err(|e| {
            GatewayError::AssemblyAI(format!("failed to parse transcript response: {e}"))
        })?;

        Ok(transcript_resp.id)
    }

    /// Poll for a transcript result.
    ///
    /// Returns the full response including status, text, and audio_duration.
    pub async fn get_transcript(&self, id: &str) -> Result<TranscriptResponse, GatewayError> {
        let url = format!("{BASE_URL}/transcript/{id}");

        let resp = self
            .http
            .get(&url)
            .header("Authorization", &self.api_key)
            .send()
            .await
            .map_err(|e| {
                GatewayError::AssemblyAI(format!("get transcript request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(GatewayError::AssemblyAI(format!(
                "getting transcript failed with status {status}: {body}"
            )));
        }

        let transcript_resp: TranscriptResponse = resp.json().await.map_err(|e| {
            GatewayError::AssemblyAI(format!("failed to parse transcript response: {e}"))
        })?;

        Ok(transcript_resp)
    }
}

/// Group flat `CustomSpelling` entries by their `to` value into AssemblyAI's
/// expected format where each entry has `from: [variant1, variant2, ...]` and `to: "word"`.
fn group_spelling(spellings: &[CustomSpelling]) -> Vec<AssemblyAISpelling> {
    use std::collections::HashMap;

    let mut groups: HashMap<&str, Vec<String>> = HashMap::new();
    // Use a Vec to preserve insertion order for deterministic output.
    let mut order: Vec<&str> = Vec::new();

    for s in spellings {
        let entry = groups.entry(s.to.as_str()).or_insert_with(|| {
            order.push(s.to.as_str());
            Vec::new()
        });
        entry.push(s.from.clone());
    }

    order
        .into_iter()
        .map(|to| AssemblyAISpelling {
            from: groups.remove(to).unwrap_or_default(),
            to: to.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_spelling_empty() {
        let result = group_spelling(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_group_spelling_single() {
        let spellings = vec![CustomSpelling {
            from: "dovac".to_string(),
            to: "Dvorak".to_string(),
        }];
        let result = group_spelling(&spellings);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].to, "Dvorak");
        assert_eq!(result[0].from, vec!["dovac"]);
    }

    #[test]
    fn test_group_spelling_multiple_from() {
        let spellings = vec![
            CustomSpelling {
                from: "dovac".to_string(),
                to: "Dvorak".to_string(),
            },
            CustomSpelling {
                from: "dvorack".to_string(),
                to: "Dvorak".to_string(),
            },
            CustomSpelling {
                from: "omarchee".to_string(),
                to: "Omarchy".to_string(),
            },
        ];
        let result = group_spelling(&spellings);
        assert_eq!(result.len(), 2);
        // First group: Dvorak
        assert_eq!(result[0].to, "Dvorak");
        assert_eq!(result[0].from, vec!["dovac", "dvorack"]);
        // Second group: Omarchy
        assert_eq!(result[1].to, "Omarchy");
        assert_eq!(result[1].from, vec!["omarchee"]);
    }
}
