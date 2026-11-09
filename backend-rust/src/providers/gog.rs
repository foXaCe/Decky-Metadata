//! GOG as a localized description provider.
//!
//! GOG's catalog and product endpoints are public, keyless, and answer in the
//! requested locale, so PC titles sold there get a store-written description in
//! the reader's language instead of the English IGN fallback.

use crate::{
    http::Http,
    matching::{clean_game_title, description_html_to_text, py_string},
};
use reqwest::Url;
use std::time::Duration;

const SEARCH_URL: &str = "https://catalog.gog.com/v1/catalog";
const PRODUCT_URL: &str = "https://api.gog.com/products";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_LIMIT: usize = 512 * 1024;
const SEARCH_LIMIT: usize = 5;

/// GOG locale identifiers for a Steam content language.
pub fn gog_locale(language: &str) -> &'static str {
    match language {
        "english" => "en-US",
        "french" => "fr-FR",
        "german" => "de-DE",
        "spanish" => "es-ES",
        "latam" => "es-419",
        "italian" => "it-IT",
        "portuguese" => "pt-PT",
        "brazilian" => "pt-BR",
        "polish" => "pl-PL",
        "russian" => "ru-RU",
        "schinese" => "zh-Hans",
        "tchinese" => "zh-Hant",
        "japanese" => "ja-JP",
        "koreana" => "ko-KR",
        "turkish" => "tr-TR",
        "dutch" => "nl-NL",
        "czech" => "cs-CZ",
        "hungarian" => "hu-HU",
        "romanian" => "ro-RO",
        "ukrainian" => "uk-UA",
        "bulgarian" => "bg-BG",
        "greek" => "el-GR",
        "thai" => "th-TH",
        "vietnamese" => "vi-VN",
        "indonesian" => "id-ID",
        _ => "en-US",
    }
}

pub struct LocalizedDescription {
    pub title: String,
    pub url: String,
    pub text: String,
}

/// Look the game up on GOG and return the store description in `locale`.
pub async fn localized_description(
    http: &Http,
    locale: &str,
    game_title: &str,
) -> Option<LocalizedDescription> {
    let title = clean_game_title(game_title);
    if title.chars().count() < 2 {
        return None;
    }
    let product = search(http, locale, &title).await?;
    let details = product_details(http, locale, &product.id).await?;
    let text = description_html_to_text(&details.lead);
    if text.chars().count() < 40 {
        return None;
    }
    Some(LocalizedDescription {
        url: format!("https://www.gog.com/game/{}", product.slug),
        title: if details.title.is_empty() {
            product.title
        } else {
            details.title
        },
        text,
    })
}

struct Candidate {
    id: String,
    title: String,
    slug: String,
}

struct Details {
    title: String,
    lead: String,
}

fn catalog_url(locale: &str, title: &str) -> String {
    let mut url = Url::parse(SEARCH_URL).expect("fixed catalog URL");
    url.query_pairs_mut()
        .append_pair("query", &format!("like:{title}"))
        .append_pair("limit", &SEARCH_LIMIT.to_string())
        .append_pair("locale", locale);
    url.to_string()
}

async fn search(http: &Http, locale: &str, title: &str) -> Option<Candidate> {
    let payload = http
        .json(&catalog_url(locale, title), REQUEST_TIMEOUT, REQUEST_LIMIT)
        .await
        .ok()?;
    let products = payload["products"].as_array()?;
    let target = normalize(title);
    let mut best: Option<(f64, Candidate)> = None;
    for product in products {
        let candidate_title = py_string(&product["title"]);
        if candidate_title.is_empty() {
            continue;
        }
        let score = title_score(&normalize(&candidate_title), &target);
        if score < 0.75 {
            continue;
        }
        let slug = py_string(&product["slug"]);
        if slug.is_empty() {
            continue;
        }
        let candidate = Candidate {
            id: py_string(&product["id"]),
            title: candidate_title,
            slug,
        };
        if best.as_ref().is_none_or(|(best_score, _)| score > *best_score) {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

fn product_details_url(locale: &str, id: &str) -> String {
    let mut url = Url::parse(&format!("{PRODUCT_URL}/{id}")).expect("fixed product URL");
    url.query_pairs_mut()
        .append_pair("locale", locale)
        .append_pair("expand", "description");
    url.to_string()
}

async fn product_details(http: &Http, locale: &str, id: &str) -> Option<Details> {
    let payload = http
        .json(&product_details_url(locale, id), REQUEST_TIMEOUT, REQUEST_LIMIT)
        .await
        .ok()?;
    Some(Details {
        title: py_string(&payload["title"]),
        lead: py_string(&payload["description"]["lead"]),
    })
}

/// Lowercase, strip accents and punctuation, and compare "&" with "and".
fn normalize(text: &str) -> String {
    let text = text.replace('&', " and ");
    let mut normalized = String::with_capacity(text.len());
    for character in text.chars() {
        let mapped = match character.to_lowercase().next().unwrap_or(character) {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            other => other,
        };
        normalized.push(if mapped.is_alphanumeric() { mapped } else { ' ' });
    }
    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn title_score(candidate: &str, target: &str) -> f64 {
    if candidate == target {
        return 1.0;
    }
    let left: std::collections::HashSet<&str> = candidate.split_whitespace().collect();
    let right: std::collections::HashSet<&str> = target.split_whitespace().collect();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(&right).count() as f64;
    if left.len() >= 2 && left.is_subset(&right) {
        return 0.9;
    }
    intersection / left.union(&right).count() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Fixture, Http};
    use serde_json::json;

    fn catalog(locale: &str, title: &str) -> String {
        catalog_url(locale, title)
    }

    fn product(locale: &str, id: &str) -> String {
        product_details_url(locale, id)
    }

    #[tokio::test]
    async fn returns_the_localized_store_description() {
        let http = Http::fixtures(vec![
            Fixture::json(
                catalog("fr-FR", "Example Quest"),
                json!({"products": [
                    {"id": "123", "title": "Example Quest", "slug": "example_quest"},
                    {"id": "456", "title": "Example Quest 2", "slug": "example_quest_2"}
                ]}),
            ),
            Fixture::json(
                product("fr-FR", "123"),
                json!({"title": "Example Quest",
                       "description": {"lead": "<p>Example Quest est un jeu d'aventure et de réflexion dans un monde ouvert.</p>"}}),
            ),
        ]);
        let localized = localized_description(&http, "fr-FR", "Example Quest")
            .await
            .expect("a matching product");
        assert_eq!(localized.text, "Example Quest est un jeu d'aventure et de réflexion dans un monde ouvert.");
        assert_eq!(localized.url, "https://www.gog.com/game/example_quest");
        http.assert_drained();
    }

    #[tokio::test]
    async fn ignores_a_catalog_without_a_close_title() {
        let http = Http::fixtures(vec![Fixture::json(
            catalog("fr-FR", "Example Quest"),
            json!({"products": [{"id": "9", "title": "Unrelated", "slug": "unrelated"}]}),
        )]);
        assert!(localized_description(&http, "fr-FR", "Example Quest").await.is_none());
        http.assert_drained();
    }

    #[tokio::test]
    async fn skips_an_empty_description() {
        let http = Http::fixtures(vec![
            Fixture::json(
                catalog("fr-FR", "Example Quest"),
                json!({"products": [{"id": "1", "title": "Example Quest", "slug": "q"}]}),
            ),
            Fixture::json(product("fr-FR", "1"), json!({"description": {"lead": ""}})),
        ]);
        assert!(localized_description(&http, "fr-FR", "Example Quest").await.is_none());
        http.assert_drained();
    }

    #[test]
    fn compares_and_ampersand_with_and() {
        assert_eq!(normalize("Example & Friends"), normalize("Example and Friends"));
    }
}
