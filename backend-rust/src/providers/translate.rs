//! Machine-translation fallback for descriptions no provider localizes.
//!
//! MyMemory answers without a key or an account, which keeps the fallback
//! usable out of the box. It only accepts short queries, so the text is split
//! on sentence boundaries and translated chunk by chunk. The result is stored
//! on the record, so a description is translated once.

use crate::{http::Http, matching::py_string};
use reqwest::Url;
use std::time::Duration;

const ENDPOINT: &str = "https://api.mymemory.translated.net/get";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const REQUEST_LIMIT: usize = 128 * 1024;
/// MyMemory rejects queries longer than 500 characters; keep a margin.
const MAX_CHUNK_CHARS: usize = 450;
const MAX_CHUNKS: usize = 8;
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(300);

pub const PROVIDER: &str = "MyMemory";

pub struct Translation {
    pub text: String,
    pub source_language: String,
    pub target_language: String,
}

/// Translate an English description into `target_language` (two-letter code).
pub async fn translate(
    http: &Http,
    text: &str,
    target_language: &str,
) -> Option<Translation> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() < 40 || target_language == "en" {
        return None;
    }
    let mut translated = Vec::new();
    for (index, chunk) in chunks(&text).into_iter().take(MAX_CHUNKS).enumerate() {
        if index > 0 {
            tokio::time::sleep(MIN_REQUEST_INTERVAL).await;
        }
        translated.push(translate_chunk(http, &chunk, target_language).await?);
    }
    let text = translated.join(" ");
    (text.chars().count() >= 20).then_some(Translation {
        text,
        source_language: "en".into(),
        target_language: target_language.into(),
    })
}

async fn translate_chunk(http: &Http, text: &str, target_language: &str) -> Option<String> {
    let mut url = Url::parse(ENDPOINT).ok()?;
    url.query_pairs_mut()
        .append_pair("q", text)
        .append_pair("langpair", &format!("en|{target_language}"));
    let payload = match http.json(url.as_str(), REQUEST_TIMEOUT, REQUEST_LIMIT).await {
        Ok(payload) => payload,
        Err(error) => {
            crate::logging::event(
                "translate",
                "request failed",
                serde_json::json!({"error": error}),
                log::Level::Warn,
            );
            return None;
        }
    };
    if payload["responseStatus"].as_i64().is_some_and(|status| status != 200) {
        crate::logging::event(
            "translate",
            "provider rejected a chunk",
            serde_json::json!({"details": py_string(&payload["responseDetails"])}),
            log::Level::Warn,
        );
        return None;
    }
    let text = py_string(&payload["responseData"]["translatedText"]);
    (!text.trim().is_empty()).then_some(text)
}

/// Split a description into queries MyMemory accepts, preferring sentence ends.
fn chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate_len = current.chars().count() + word.chars().count() + 1;
        if candidate_len > MAX_CHUNK_CHARS && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
        // Close a chunk at a sentence end once it is reasonably sized.
        if current.chars().count() > MAX_CHUNK_CHARS / 2
            && word.ends_with(['.', '!', '?'])
        {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Fixture, Http};

    fn api_url(text: &str, target: &str) -> String {
        let mut url = Url::parse(ENDPOINT).unwrap();
        url.query_pairs_mut()
            .append_pair("q", text)
            .append_pair("langpair", &format!("en|{target}"));
        url.to_string()
    }

    fn reply(text: &str) -> serde_json::Value {
        serde_json::json!({"responseStatus": 200, "responseData": {"translatedText": text}})
    }

    #[test]
    fn splits_long_text_into_bounded_chunks() {
        let text = "Phrase de test assez longue pour remplir. ".repeat(40);
        let chunks = chunks(&text);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= MAX_CHUNK_CHARS));
        assert_eq!(chunks.join(" "), text.split_whitespace().collect::<Vec<_>>().join(" "));
    }

    #[tokio::test]
    async fn translates_a_short_description_in_one_query() {
        let text = "The hero must save the kingdom from the darkness spreading from the castle.";
        let http = Http::fixtures(vec![Fixture::json(
            api_url(text, "fr"),
            reply("Le héros doit sauver le royaume des ténèbres."),
        )]);
        let translation = translate(&http, text, "fr").await.unwrap();
        assert_eq!(translation.text, "Le héros doit sauver le royaume des ténèbres.");
        assert_eq!(translation.target_language, "fr");
        http.assert_drained();
    }

    #[tokio::test]
    async fn reports_failure_instead_of_half_translated_text() {
        let text = "The evil wizard is scheming to steal the relic.";
        let http = Http::fixtures(vec![Fixture::json(
            api_url(text, "fr"),
            serde_json::json!({"responseStatus": 403, "responseDetails": "QUERY LENGTH LIMIT EXCEEDED"}),
        )]);
        assert!(translate(&http, text, "fr").await.is_none());
        http.assert_drained();
    }

    #[tokio::test]
    async fn skips_english_targets_and_short_text() {
        let http = Http::fixtures(vec![]);
        assert!(translate(&http, "A long enough English description for a game.", "en").await.is_none());
        assert!(translate(&http, "Too short", "fr").await.is_none());
        http.assert_drained();
    }
}
