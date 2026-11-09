//! Wikipedia as a localized description provider.
//!
//! Steam localizes store text when the publisher supplied it. Titles without a
//! Steam page (or without a localized one) only have the English IGN fallback.
//! Wikipedia articles are already written in the reader's language, so this
//! provider fills that gap without a translation service: no key, no hosting,
//! and no text leaves the reader's machine except a public title lookup.
//!
//! Only the article intro (`exintro`) is used, and the article URL is kept in
//! the record for attribution (Wikipedia text is CC BY-SA).

use crate::{
    http::Http,
    matching::{clean_game_title, py_string},
};
use reqwest::Url;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const REQUEST_LIMIT: usize = 256 * 1024;
const MAX_EXTRACT_CHARS: usize = 1200;
const SEARCH_LIMIT: usize = 5;
/// At most two extracts are fetched per game: a series article is rejected and
/// the next candidate is tried once.
const MAX_EXTRACT_ATTEMPTS: usize = 2;

pub enum Lookup {
    Found(LocalizedIntro),
    /// The lookup ran and no usable article exists for this title.
    NotFound,
    /// A request failed; the caller must not remember the attempt.
    Unavailable,
}

pub struct LocalizedIntro {
    pub title: String,
    pub url: String,
    pub extract: String,
}

/// Wikipedia subdomain for a Steam language, or `None` when English is the
/// target (the English fallback already covers it).
pub fn wikipedia_code(language: &str) -> Option<&'static str> {
    Some(match language {
        "english" => return None,
        "schinese" | "tchinese" => "zh",
        "brazilian" => "pt",
        "latam" => "es",
        "norwegian" => "no",
        "koreana" => "ko",
        "japanese" => "ja",
        "russian" => "ru",
        "ukrainian" => "uk",
        "polish" => "pl",
        "turkish" => "tr",
        "czech" => "cs",
        "danish" => "da",
        "dutch" => "nl",
        "finnish" => "fi",
        "swedish" => "sv",
        "hungarian" => "hu",
        "romanian" => "ro",
        "bulgarian" => "bg",
        "greek" => "el",
        "thai" => "th",
        "vietnamese" => "vi",
        "indonesian" => "id",
        "german" => "de",
        "french" => "fr",
        "italian" => "it",
        "spanish" => "es",
        "portuguese" => "pt",
        _ => return None,
    })
}

/// Localized "video game" term, appended to the search so disambiguated game
/// articles rank next to a same-named series or film page.
const GAME_TERMS: &[(&str, &str)] = &[
    ("fr", "jeu vidéo"),
    ("de", "Computerspiel"),
    ("es", "videojuego"),
    ("it", "videogioco"),
    ("pt", "jogo eletrônico"),
    ("ja", "コンピュータゲーム"),
    ("ru", "компьютерная игра"),
    ("uk", "відеогра"),
    ("pl", "gra komputerowa"),
    ("nl", "computerspel"),
    ("tr", "video oyunu"),
    ("sv", "datorspel"),
    ("da", "computerspil"),
    ("fi", "videopeli"),
    ("no", "videospill"),
    ("cs", "počítačová hra"),
    ("hu", "videojáték"),
    ("ro", "joc video"),
    ("bg", "видеоигра"),
    ("el", "βιντεοπαιχνίδι"),
    ("th", "วิดีโอเกม"),
    ("vi", "trò chơi điện tử"),
    ("id", "permainan video"),
    ("zh", "电子游戏"),
    ("ko", "비디오 게임"),
];

fn game_term(code: &str) -> Option<&'static str> {
    GAME_TERMS
        .iter()
        .find(|(language, _)| *language == code)
        .map(|(_, term)| *term)
}

/// Look up an article in the reader's language and return its intro.
pub async fn localized_intro(http: &Http, language: &str, game_title: &str) -> Lookup {
    let Some(code) = wikipedia_code(language) else {
        return Lookup::NotFound;
    };
    let title = clean_game_title(game_title);
    if title.chars().count() < 2 {
        return Lookup::NotFound;
    }
    let query = match game_term(code) {
        Some(term) => format!("{title} {term}"),
        None => title.clone(),
    };
    let candidates = match search(http, code, &query).await {
        Ok(candidates) => candidates,
        Err(()) => return Lookup::Unavailable,
    };
    let mut request_failed = false;
    for candidate in ranked(candidates, &title)
        .into_iter()
        .take(MAX_EXTRACT_ATTEMPTS)
    {
        let extract = match extract(http, code, &candidate).await {
            Ok(extract) => extract,
            Err(()) => {
                request_failed = true;
                continue;
            }
        };
        let Some(extract) = extract else {
            continue;
        };
        let intro = trim_intro(&extract);
        if !looks_like_game(&intro) {
            continue;
        }
        return Lookup::Found(LocalizedIntro {
            url: article_url(code, &candidate),
            title: candidate,
            extract: intro,
        });
    }
    if request_failed {
        Lookup::Unavailable
    } else {
        Lookup::NotFound
    }
}

/// Confirm that an article is about a video game through Wikidata
/// (`instance of` = Q7889). Used before creating a record from Wikipedia so a
/// tool or a service that happens to have an article is never saved as a game.
pub async fn is_video_game(http: &Http, code: &str, article_title: &str) -> Option<bool> {
    let mut url = Url::parse("https://www.wikidata.org/w/api.php").ok()?;
    url.query_pairs_mut()
        .append_pair("action", "wbgetentities")
        .append_pair("sites", &format!("{code}wiki"))
        .append_pair("titles", article_title)
        .append_pair("props", "claims")
        .append_pair("format", "json")
        .append_pair("utf8", "1");
    let payload = http
        .json(url.as_str(), REQUEST_TIMEOUT, REQUEST_LIMIT)
        .await
        .ok()?;
    let entity = payload["entities"]
        .as_object()
        .and_then(|entities| entities.values().next())?;
    let claims = entity["claims"]["P31"].as_array()?;
    Some(claims.iter().any(|claim| {
        claim["mainsnak"]["datavalue"]["value"]["id"].as_str() == Some("Q7889")
    }))
}

async fn search(http: &Http, code: &str, title: &str) -> Result<Vec<String>, ()> {
    let mut url = Url::parse(&format!("https://{code}.wikipedia.org/w/api.php")).map_err(|_| ())?;
    url.query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("list", "search")
        .append_pair("srsearch", title)
        .append_pair("srlimit", &SEARCH_LIMIT.to_string())
        .append_pair("format", "json")
        .append_pair("utf8", "1");
    let payload = http
        .json(url.as_str(), REQUEST_TIMEOUT, REQUEST_LIMIT)
        .await
        .map_err(|_| ())?;
    let items = payload["query"]["search"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .map(|item| py_string(&item["title"]))
        .filter(|title| !title.is_empty())
        .collect())
}

async fn extract(http: &Http, code: &str, title: &str) -> Result<Option<String>, ()> {
    let mut url = Url::parse(&format!("https://{code}.wikipedia.org/w/api.php")).map_err(|_| ())?;
    url.query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("prop", "extracts")
        .append_pair("exintro", "1")
        .append_pair("explaintext", "1")
        .append_pair("redirects", "1")
        .append_pair("format", "json")
        .append_pair("utf8", "1")
        .append_pair("titles", title);
    let payload = http
        .json(url.as_str(), REQUEST_TIMEOUT, REQUEST_LIMIT)
        .await
        .map_err(|_| ())?;
    let Some(pages) = payload["query"]["pages"].as_object() else {
        return Ok(None);
    };
    Ok(pages
        .values()
        .find_map(|page| page["extract"].as_str().map(str::to_owned)))
}

/// Order candidates by title similarity, then by plain-title preference.
fn ranked(mut candidates: Vec<String>, game_title: &str) -> Vec<String> {
    let target = normalize(game_title);
    let mut scored: Vec<(f64, bool, usize, String)> = candidates
        .drain(..)
        .enumerate()
        .filter_map(|(index, candidate)| {
            if has_negative_hint(&candidate) {
                return None;
            }
            let score = title_score(&candidate, &target);
            (score >= 0.75).then_some((score, candidate.contains('('), index, candidate))
        })
        .collect();
    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    scored.into_iter().map(|(_, _, _, title)| title).collect()
}

fn title_score(candidate: &str, target: &str) -> f64 {
    let base = normalize(candidate);
    if base == target {
        return 1.0;
    }
    let left: std::collections::HashSet<&str> = base.split_whitespace().collect();
    let right: std::collections::HashSet<&str> = target.split_whitespace().collect();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(&right).count() as f64;
    let union = left.union(&right).count() as f64;
    intersection / union
}

/// Lowercase, strip accents, drop parenthetical disambiguation and punctuation.
fn normalize(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut depth = 0usize;
    for character in text.chars() {
        match character {
            '(' => {
                depth += 1;
                continue;
            }
            ')' => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        if depth > 0 {
            continue;
        }
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

/// Articles about a film, series, book, or list must not replace a game
/// description.
fn has_negative_hint(title: &str) -> bool {
    let lowered = title.to_lowercase();
    ["film", "série", "serie", "series", "saga", "franchise", "manga", "roman",
     "novel", "movie", "soundtrack", "bande originale", "album", "personnage",
     "character", "liste", "list of", "console"]
        .iter()
        .any(|hint| lowered.contains(hint))
}

/// The intro must describe a game and must not be about a series.
fn looks_like_game(intro: &str) -> bool {
    let opening: String = intro.chars().take(90).collect::<String>().to_lowercase();
    // Foreign titles and parentheticals can push the genre well past the first
    // line, so the game keyword is searched in the whole lead paragraph.
    let head: String = intro.chars().take(600).collect::<String>().to_lowercase();
    if ["série", "serie", "series", "franchise", "saga"]
        .iter()
        .any(|hint| opening.contains(hint))
    {
        return false;
    }
    ["jeu", "game", "spiel", "juego", "videogioco", "jogo", "gioco", "игр",
     "游戏", "ゲーム", "gra", "hra", "spel", "oyun", "játék", "joc", "peli",
     "igra", "βιντεοπαιχνίδι"]
        .iter()
        .any(|hint| head.contains(hint))
}

fn trim_intro(extract: &str) -> String {
    let collapsed = extract.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_EXTRACT_CHARS {
        return collapsed;
    }
    let mut result: String = collapsed.chars().take(MAX_EXTRACT_CHARS).collect();
    // Prefer a sentence boundary in the last third of the excerpt.
    if let Some(index) = result.rfind(". ") {
        if index > MAX_EXTRACT_CHARS / 3 {
            result.truncate(index + 1);
        }
    }
    result
}

fn article_url(code: &str, title: &str) -> String {
    format!(
        "https://{code}.wikipedia.org/wiki/{}",
        encode_path(&title.replace(' ', "_"))
    )
}

fn encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Fixture, Http};
    use reqwest::Url;
    use serde_json::{json, Value};

    fn wiki_url(code: &str, pairs: &[(&str, &str)]) -> String {
        let mut url = Url::parse(&format!("https://{code}.wikipedia.org/w/api.php")).unwrap();
        for (key, value) in pairs {
            url.query_pairs_mut().append_pair(key, value);
        }
        url.to_string()
    }

    fn search_url(code: &str, query: &str) -> String {
        wiki_url(
            code,
            &[
                ("action", "query"),
                ("list", "search"),
                ("srsearch", query),
                ("srlimit", "5"),
                ("format", "json"),
                ("utf8", "1"),
            ],
        )
    }

    fn extract_url(code: &str, title: &str) -> String {
        wiki_url(
            code,
            &[
                ("action", "query"),
                ("prop", "extracts"),
                ("exintro", "1"),
                ("explaintext", "1"),
                ("redirects", "1"),
                ("format", "json"),
                ("utf8", "1"),
                ("titles", title),
            ],
        )
    }

    fn search_payload(titles: &[&str]) -> Value {
        json!({"query": {"search": titles.iter().map(|title| json!({"title": title})).collect::<Vec<_>>()}})
    }

    fn extract_payload(title: &str, extract: &str) -> Value {
        json!({"query": {"pages": {"1": {"title": title, "extract": extract}}}})
    }

    #[test]
    fn maps_steam_languages_to_wikipedia_subdomains() {
        assert_eq!(wikipedia_code("french"), Some("fr"));
        assert_eq!(wikipedia_code("german"), Some("de"));
        assert_eq!(wikipedia_code("schinese"), Some("zh"));
        assert_eq!(wikipedia_code("brazilian"), Some("pt"));
        assert_eq!(wikipedia_code("english"), None);
        assert_eq!(wikipedia_code("klingon"), None);
    }

    #[tokio::test]
    async fn returns_the_localized_intro_and_article_url() {
        let http = Http::fixtures(vec![
            Fixture::json(
                search_url("fr", "Neutral Racer jeu vidéo"),
                search_payload(&["Neutral Racer"]),
            ),
            Fixture::json(
                extract_url("fr", "Neutral Racer"),
                extract_payload(
                    "Neutral Racer",
                    "Neutral Racer est un jeu vidéo de course automobile développé par un studio d'exemple.",
                ),
            ),
        ]);
        let Lookup::Found(intro) = localized_intro(&http, "french", "Neutral Racer").await else {
            panic!("expected a localized intro");
        };
        assert_eq!(intro.title, "Neutral Racer");
        assert_eq!(intro.url, "https://fr.wikipedia.org/wiki/Neutral_Racer");
        assert!(intro.extract.starts_with("Neutral Racer est un jeu vidéo"));
        http.assert_drained();
    }

    #[tokio::test]
    async fn rejects_a_series_article_and_tries_the_next_candidate() {
        let http = Http::fixtures(vec![
            Fixture::json(
                search_url("fr", "Example Quest jeu vidéo"),
                search_payload(&["Example Quest", "Example Quest (jeu vidéo)"]),
            ),
            Fixture::json(
                extract_url("fr", "Example Quest"),
                extract_payload("Example Quest", "Example Quest est une série de jeux vidéo."),
            ),
            Fixture::json(
                extract_url("fr", "Example Quest (jeu vidéo)"),
                extract_payload(
                    "Example Quest (jeu vidéo)",
                    "Example Quest est un jeu vidéo d'aventure sorti en 1998.",
                ),
            ),
        ]);
        let Lookup::Found(intro) = localized_intro(&http, "french", "Example Quest").await else {
            panic!("expected a localized intro");
        };
        assert_eq!(intro.title, "Example Quest (jeu vidéo)");
        assert!(intro.url.contains("Example_Quest_"));
        http.assert_drained();
    }

    #[tokio::test]
    async fn skips_non_game_candidates() {
        let http = Http::fixtures(vec![Fixture::json(
            search_url("fr", "Example Quest jeu vidéo"),
            search_payload(&["Example Quest (film)"]),
        )]);
        assert!(matches!(
            localized_intro(&http, "french", "Example Quest").await,
            Lookup::NotFound
        ));
        http.assert_drained();
    }

    #[tokio::test]
    async fn does_not_call_wikipedia_for_english() {
        let http = Http::fixtures(vec![]);
        assert!(matches!(
            localized_intro(&http, "english", "Neutral Racer").await,
            Lookup::NotFound
        ));
        http.assert_drained();
    }

    #[tokio::test]
    async fn ignores_unrelated_search_results() {
        let http = Http::fixtures(vec![Fixture::json(
            search_url("fr", "Neutral Racer jeu vidéo"),
            search_payload(&["Unrelated Article", "Another Subject"]),
        )]);
        assert!(matches!(
            localized_intro(&http, "french", "Neutral Racer").await,
            Lookup::NotFound
        ));
        http.assert_drained();
    }

    #[tokio::test]
    async fn reports_a_failed_request_as_unavailable() {
        let http = Http::fixtures(vec![Fixture::failure(search_url(
            "fr",
            "Neutral Racer jeu vidéo",
        ))]);
        assert!(matches!(
            localized_intro(&http, "french", "Neutral Racer").await,
            Lookup::Unavailable
        ));
        http.assert_drained();
    }

    #[tokio::test]
    async fn wikidata_confirms_whether_an_article_is_a_video_game() {
        let url = |title: &str| {
            let mut url = Url::parse("https://www.wikidata.org/w/api.php").unwrap();
            url.query_pairs_mut()
                .append_pair("action", "wbgetentities")
                .append_pair("sites", "frwiki")
                .append_pair("titles", title)
                .append_pair("props", "claims")
                .append_pair("format", "json")
                .append_pair("utf8", "1");
            url.to_string()
        };
        let claims = |id: &str| json!({"entities": {"Q1": {"claims": {"P31": [
            {"mainsnak": {"datavalue": {"value": {"id": id}}}}
        ]}}}});
        let game = Http::fixtures(vec![Fixture::json(url("Example Quest"), claims("Q7889"))]);
        assert_eq!(is_video_game(&game, "fr", "Example Quest").await, Some(true));
        game.assert_drained();

        let software = Http::fixtures(vec![Fixture::json(url("Example Tool"), claims("Q7397"))]);
        assert_eq!(is_video_game(&software, "fr", "Example Tool").await, Some(false));
        software.assert_drained();

        let failed = Http::fixtures(vec![Fixture::failure(url("Example Quest"))]);
        assert_eq!(is_video_game(&failed, "fr", "Example Quest").await, None);
        failed.assert_drained();
    }

    #[test]
    fn trims_long_intros_on_a_sentence_boundary() {
        let long = "Phrase de test. ".repeat(200);
        let trimmed = trim_intro(&long);
        assert!(trimmed.chars().count() <= MAX_EXTRACT_CHARS);
        assert!(trimmed.ends_with('.'));
    }

    #[test]
    fn encodes_article_urls() {
        assert_eq!(
            article_url("fr", "Exemple: jeu vidéo"),
            "https://fr.wikipedia.org/wiki/Exemple%3A_jeu_vid%C3%A9o"
        );
    }
}
