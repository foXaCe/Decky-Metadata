//! SteamGridDB artwork provider.
//!
//! Uses the public SteamGridDB API with the user's own key (the one shipped by
//! the upstream plugin is reserved for that plugin and gets other clients
//! banned). Images are returned as base64 so the frontend can hand them to
//! Steam's `SetCustomArtworkForApp`, which writes the files and invalidates the
//! library cache for us.

use crate::http::{Http, Redirects};
use crate::matching::{clean_game_title, py_string};
use reqwest::Method;
use serde_json::{json, Value};
use std::time::Duration;

const API_BASE: &str = "https://www.steamgriddb.com/api/v2";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const JSON_LIMIT: usize = 1024 * 1024;
const IMAGE_LIMIT: usize = 12 * 1024 * 1024;

pub struct Artwork {
    pub portrait: Option<String>,
    pub landscape: Option<String>,
    pub hero: Option<String>,
    pub logo: Option<String>,
}

/// Resolve the game and return the best base64 image for each Steam artwork slot.
pub async fn fetch(
    http: &Http,
    api_key: &str,
    title: &str,
    steam_appid: Option<i64>,
) -> Option<Artwork> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return None;
    }
    let game_id = resolve_game(http, api_key, title, steam_appid).await?;
    let grids = assets(http, api_key, &format!("/grids/game/{game_id}?nsfw=false&humor=false&epilepsy=false")).await;
    let heroes = assets(http, api_key, &format!("/heroes/game/{game_id}?nsfw=false&humor=false&epilepsy=false")).await;
    let logos = assets(http, api_key, &format!("/logos/game/{game_id}?nsfw=false&humor=false")).await;

    let portrait = pick(&grids, |width, height| height > width && ratio(width, height, 2.0 / 3.0, 0.08));
    let landscape = pick(&grids, |width, height| width > height && ratio(width, height, 920.0 / 430.0, 0.25));
    let hero = pick(&heroes, |_, _| true);
    let logo = pick(&logos, |_, _| true);

    Some(Artwork {
        portrait: download(http, api_key, portrait.as_deref()).await,
        landscape: download(http, api_key, landscape.as_deref()).await,
        hero: download(http, api_key, hero.as_deref()).await,
        logo: download(http, api_key, logo.as_deref()).await,
    })
}

async fn resolve_game(
    http: &Http,
    api_key: &str,
    title: &str,
    steam_appid: Option<i64>,
) -> Option<i64> {
    if let Some(appid) = steam_appid.filter(|id| *id > 0) {
        if let Ok(payload) = get(http, api_key, &format!("/games/steam/{appid}")).await {
            if let Some(id) = payload["data"]["id"].as_i64() {
                return Some(id);
            }
        }
    }
    let title = clean_game_title(title);
    if title.chars().count() < 2 {
        return None;
    }
    let url = format!("{API_BASE}/search/autocomplete/{}", encode(&title));
    let payload = get_url(http, api_key, &url).await.ok()?;
    let results = payload["data"].as_array()?;
    let target = normalize(&title);
    results.iter().find_map(|game| {
        let name = normalize(&py_string(&game["name"]));
        let score = score(&name, &target);
        (score >= 0.8).then(|| game["id"].as_i64()).flatten()
    })
}

async fn assets(http: &Http, api_key: &str, path: &str) -> Vec<Value> {
    let Ok(payload) = get(http, api_key, path).await else {
        return Vec::new();
    };
    payload["data"].as_array().cloned().unwrap_or_default()
}

fn pick(assets: &[Value], accept: impl Fn(i64, i64) -> bool) -> Option<String> {
    assets.iter().find_map(|asset| {
        let url = py_string(&asset["url"]);
        if url.is_empty() {
            return None;
        }
        let width = asset["width"].as_i64().unwrap_or(0);
        let height = asset["height"].as_i64().unwrap_or(0);
        accept(width, height).then_some(url)
    })
}

async fn download_bytes(http: &Http, api_key: &str, url: &str) -> Option<Vec<u8>> {
    let response = http
        .request(
            Method::GET,
            url,
            &[("Authorization", &format!("Bearer {api_key}"))],
            None,
            REQUEST_TIMEOUT,
            IMAGE_LIMIT,
            Redirects::Default,
        )
        .await
        .ok()?;
    if !(200..300).contains(&response.status) || response.body.is_empty() {
        return None;
    }
    Some(response.body)
}

async fn download(http: &Http, api_key: &str, url: Option<&str>) -> Option<String> {
    let url = url?;
    download_bytes(http, api_key, url).await.map(|body| base64(&body))
}

async fn get(http: &Http, api_key: &str, path: &str) -> Result<Value, ()> {
    let url = format!("{API_BASE}{path}");
    get_url(http, api_key, &url).await
}

async fn get_url(http: &Http, api_key: &str, url: &str) -> Result<Value, ()> {
    let response = http
        .request(
            Method::GET,
            url,
            &[("Authorization", &format!("Bearer {api_key}"))],
            None,
            REQUEST_TIMEOUT,
            JSON_LIMIT,
            Redirects::Default,
        )
        .await
        .map_err(|_| ())?;
    if !(200..300).contains(&response.status) {
        return Err(());
    }
    let payload: Value = response.json().map_err(|_| ())?;
    if payload["success"].as_bool() == Some(false) {
        return Err(());
    }
    Ok(payload)
}

fn ratio(width: i64, height: i64, expected: f64, tolerance: f64) -> bool {
    if width <= 0 || height <= 0 {
        return false;
    }
    ((width as f64 / height as f64) - expected).abs() <= tolerance
}

fn normalize(text: &str) -> String {
    text.replace('&', " and ")
        .chars()
        .map(|character| {
            let lower = character.to_lowercase().next().unwrap_or(character);
            match lower {
                'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
                'ç' => 'c',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ñ' => 'n',
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ý' | 'ÿ' => 'y',
                other => other,
            }
        })
        .map(|character| if character.is_alphanumeric() { character } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn score(candidate: &str, target: &str) -> f64 {
    if candidate == target {
        return 1.0;
    }
    let left: std::collections::HashSet<&str> = candidate.split_whitespace().collect();
    let right: std::collections::HashSet<&str> = target.split_whitespace().collect();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left.len() >= 2 && left.is_subset(&right) {
        return 0.9;
    }
    left.intersection(&right).count() as f64 / left.union(&right).count() as f64
}

fn encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        out.push(TABLE[(first >> 2) as usize] as char);
        out.push(TABLE[(((first & 0b11) << 4) | (second >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((second & 0b1111) << 2) | (third >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(third & 0b111111) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Every candidate per Steam slot, for the artwork gallery. Only metadata is
/// returned; images are downloaded one at a time once the user picks one.
pub async fn candidates(
    http: &Http,
    api_key: &str,
    title: &str,
    steam_appid: Option<i64>,
) -> Value {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return json!({});
    }
    let Some(game_id) = resolve_game(http, api_key, title, steam_appid).await else {
        return json!({});
    };
    let grids = assets(http, api_key, &format!("/grids/game/{game_id}?nsfw=false&humor=false&epilepsy=false")).await;
    let heroes = assets(http, api_key, &format!("/heroes/game/{game_id}?nsfw=false&humor=false&epilepsy=false")).await;
    let logos = assets(http, api_key, &format!("/logos/game/{game_id}?nsfw=false&humor=false")).await;

    let mut payload = serde_json::Map::new();
    payload.insert(
        "portrait".into(),
        listing(&grids, |width, height| height > width && ratio(width, height, 2.0 / 3.0, 0.08)),
    );
    payload.insert(
        "landscape".into(),
        listing(&grids, |width, height| width > height && ratio(width, height, 920.0 / 430.0, 0.25)),
    );
    let icons = assets(http, api_key, &format!("/icons/game/{game_id}?nsfw=false&humor=false")).await;
    payload.insert("hero".into(), listing(&heroes, |_, _| true));
    payload.insert("logo".into(), listing(&logos, |_, _| true));
    // Shortcut icons are stored as the image Steam will read: keep raster
    // formats it understands and prefer PNG.
    let mut icon_listing = listing(&icons, |_, _| true);
    if let Some(items) = icon_listing.as_array_mut() {
        items.sort_by_key(|asset| {
            let url = py_string(&asset["url"]).to_lowercase();
            (!url.ends_with(".png")) as u8
        });
        items.retain(|asset| {
            let url = py_string(&asset["url"]).to_lowercase();
            [".png", ".jpg", ".jpeg", ".webp"]
                .iter()
                .any(|extension| url.split('?').next().unwrap_or(&url).ends_with(extension))
        });
    }
    payload.insert("icon".into(), icon_listing);
    Value::Object(payload)
}

fn listing(assets: &[Value], accept: impl Fn(i64, i64) -> bool) -> Value {
    Value::Array(
        assets
            .iter()
            .filter(|asset| {
                let width = asset["width"].as_i64().unwrap_or(0);
                let height = asset["height"].as_i64().unwrap_or(0);
                accept(width, height)
            })
            .take(8)
            .filter_map(|asset| {
                let url = py_string(&asset["url"]);
                if url.is_empty() {
                    return None;
                }
                Some(json!({
                    "id": asset["id"].clone(),
                    "url": url,
                    "thumb": py_string(&asset["thumb"]),
                    "width": asset["width"].clone(),
                    "height": asset["height"].clone(),
                    "style": py_string(&asset["style"]),
                }))
            })
            .collect(),
    )
}

/// Download one gallery image; only SteamGridDB hosts are accepted.
pub async fn download_url(http: &Http, api_key: &str, url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?;
    if !host.ends_with(".steamgriddb.com") && host != "steamgriddb.com" {
        return None;
    }
    download(http, api_key.trim(), Some(url)).await
}

/// Install a shortcut icon and point Steam's shortcuts.vdf at it.
///
/// Icons are not part of Steam's custom-artwork API for shortcuts, so this
/// writes the image next to the other artwork and replaces only the `icon`
/// value of the matching shortcut. Steam reads it after a restart.
pub async fn apply_shortcut_icon(
    http: &Http,
    api_key: &str,
    app_id: u64,
    url: &str,
) -> Result<bool, String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    let host = parsed.host_str().unwrap_or_default();
    if parsed.scheme() != "https" || (!host.ends_with(".steamgriddb.com") && host != "steamgriddb.com") {
        return Err("artwork host is not SteamGridDB".to_owned());
    }
    let bytes = download_bytes(http, api_key.trim(), url)
        .await
        .ok_or_else(|| "icon download failed".to_owned())?;
    let shortcuts = crate::steam_paths::detect_steam_installs(&crate::steam_paths::discover_steam_roots())
        .into_iter()
        .flat_map(|install| install.userdata_dirs)
        .map(|dir| dir.join("config").join("shortcuts.vdf"))
        .find(|path| path.is_file())
        .ok_or_else(|| "shortcuts.vdf not found".to_owned())?;
    let extension = parsed
        .path()
        .rsplit('.')
        .next()
        .map(str::to_lowercase)
        .filter(|extension| ["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()))
        .ok_or_else(|| "unsupported icon format".to_owned())?;
    let grid = shortcuts
        .parent()
        .ok_or_else(|| "invalid shortcuts path".to_owned())?
        .join("grid");
    std::fs::create_dir_all(&grid).map_err(|error| error.to_string())?;
    let icon_path = grid.join(format!("{app_id}_icon.{extension}"));
    std::fs::write(&icon_path, &bytes).map_err(|error| error.to_string())?;
    let data = std::fs::read(&shortcuts).map_err(|error| error.to_string())?;
    let updated = crate::shortcuts_vdf::set_shortcut_icon(
        &data,
        app_id as u32,
        &icon_path.to_string_lossy(),
    )?;
    crate::shortcuts_vdf::write_shortcuts_vdf(&shortcuts, &updated)?;
    Ok(true)
}

/// Which artwork files Steam already stores for each shortcut.
pub fn status(app_ids: &[u64]) -> Value {
    let roots = crate::steam_paths::userdata_roots_from_candidates(
        &crate::steam_paths::discover_steam_roots(),
    );
    let grid = roots
        .iter()
        .flat_map(|root| std::fs::read_dir(root).ok().into_iter().flatten().flatten())
        .map(|entry| entry.path().join("config").join("grid"))
        .find(|path| path.is_dir());
    let mut payload = serde_json::Map::new();
    let Some(grid) = grid else {
        return Value::Object(payload);
    };
    for app_id in app_ids {
        let exists = |names: &[String]| names.iter().any(|name| grid.join(name).is_file());
        let base = app_id.to_string();
        payload.insert(
            base.clone(),
            json!({
                "portrait": exists(&[format!("{base}p.png"), format!("{base}p.jpg")]),
                "landscape": exists(&[format!("{base}.png"), format!("{base}.jpg")]),
                "hero": exists(&[format!("{base}_hero.png"), format!("{base}_hero.jpg")]),
                "logo": exists(&[format!("{base}_logo.png"), format!("{base}_logo.jpg")]),
            }),
        );
    }
    Value::Object(payload)
}

/// Payload for the frontend, without empty entries.
pub fn payload(artwork: Artwork) -> Value {
    let mut payload = serde_json::Map::new();
    for (key, value) in [
        ("portrait", artwork.portrait),
        ("landscape", artwork.landscape),
        ("hero", artwork.hero),
        ("logo", artwork.logo),
    ] {
        if let Some(value) = value {
            payload.insert(key.to_owned(), json!(value));
        }
    }
    Value::Object(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Fixture, Http};

    const GAME: &str = "https://www.steamgriddb.com/api/v2/games/steam/620";
    const GRIDS: &str =
        "https://www.steamgriddb.com/api/v2/grids/game/42?nsfw=false&humor=false&epilepsy=false";
    const HEROES: &str =
        "https://www.steamgriddb.com/api/v2/heroes/game/42?nsfw=false&humor=false&epilepsy=false";
    const LOGOS: &str =
        "https://www.steamgriddb.com/api/v2/logos/game/42?nsfw=false&humor=false";

    #[tokio::test]
    async fn requires_an_api_key_without_touching_the_network() {
        let http = Http::fixtures(vec![]);
        assert!(fetch(&http, "  ", "Portal 2", Some(620)).await.is_none());
        http.assert_drained();
    }

    #[tokio::test]
    async fn picks_one_image_per_steam_slot() {
        let http = Http::fixtures(vec![
            Fixture::json(GAME, json!({"success": true, "data": {"id": 42}})),
            Fixture::json(
                GRIDS,
                json!({"success": true, "data": [
                    {"url": "https://cdn.example/portrait.png", "width": 600, "height": 900},
                    {"url": "https://cdn.example/landscape.png", "width": 920, "height": 430}
                ]}),
            ),
            Fixture::json(
                HEROES,
                json!({"success": true, "data": [
                    {"url": "https://cdn.example/hero.png", "width": 1920, "height": 620}
                ]}),
            ),
            Fixture::json(
                LOGOS,
                json!({"success": true, "data": [
                    {"url": "https://cdn.example/logo.png", "width": 800, "height": 200}
                ]}),
            ),
            Fixture::text("https://cdn.example/portrait.png", "portrait"),
            Fixture::text("https://cdn.example/landscape.png", "landscape"),
            Fixture::text("https://cdn.example/hero.png", "hero"),
            Fixture::text("https://cdn.example/logo.png", "logo"),
        ]);
        let artwork = fetch(&http, "test-key", "Portal 2", Some(620))
            .await
            .expect("artwork for a known Steam app");
        let payload = payload(artwork);
        assert_eq!(payload["portrait"], json!(base64(b"portrait")));
        assert_eq!(payload["landscape"], json!(base64(b"landscape")));
        assert_eq!(payload["hero"], json!(base64(b"hero")));
        assert_eq!(payload["logo"], json!(base64(b"logo")));
        http.assert_drained();
    }

    #[tokio::test]
    async fn falls_back_to_a_title_search() {
        let http = Http::fixtures(vec![
            Fixture::failure(GAME),
            Fixture::json(
                "https://www.steamgriddb.com/api/v2/search/autocomplete/Example%20Quest",
                json!({"success": true, "data": [
                    {"id": 7, "name": "Unrelated"},
                    {"id": 8, "name": "Example Quest"}
                ]}),
            ),
            Fixture::json(
                "https://www.steamgriddb.com/api/v2/grids/game/8?nsfw=false&humor=false&epilepsy=false",
                json!({"success": true, "data": []}),
            ),
            Fixture::json(
                "https://www.steamgriddb.com/api/v2/heroes/game/8?nsfw=false&humor=false&epilepsy=false",
                json!({"success": true, "data": []}),
            ),
            Fixture::json(
                "https://www.steamgriddb.com/api/v2/logos/game/8?nsfw=false&humor=false",
                json!({"success": true, "data": []}),
            ),
        ]);
        let artwork = fetch(&http, "test-key", "Example Quest", Some(620))
            .await
            .expect("the game resolves by name");
        assert_eq!(payload(artwork), json!({}));
        http.assert_drained();
    }

    #[test]
    fn encodes_images_as_base64() {
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
    }
}
