use super::parser;
use crate::providers::fourkhdhub::hubcloud;
use crate::providers::models::{
    CatalogItem, MediaDetails, PlaybackSource, ProviderKind, ProviderMediaId, Release,
};
use reqwest::Url;

const DEFAULT_BASE_URL: &str = "https://hdhub4u.website/";

/// Fallback mirrors tried in order when the primary domain is unreachable.
/// The site rotates domains frequently (ISP blocks / takedowns), so the
/// provider probes each candidate until one answers.
const CANDIDATE_BASE_URLS: &[&str] = &[
    "https://hdhub4u.website/",
    "https://new1.hdhub4u.af/",
    "https://new4.hdhub4u.cl/",
];

#[derive(thiserror::Error, Debug)]
pub enum HdHub4uError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("invalid provider URL: {0}")]
    InvalidUrl(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("no playable mirror resolved")]
    NoPlayableMirror,
}

#[derive(Clone)]
pub struct HdHub4uClient {
    client: reqwest::Client,
    base_url: Url,
}

impl Default for HdHub4uClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HdHub4uClient {
    pub fn new() -> Self {
        let base =
            std::env::var("HDHUB4U_HDHUB4U_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(&base).unwrap_or_else(|_| Self {
            client: build_client(),
            base_url: Url::parse(DEFAULT_BASE_URL).expect("valid default HDHub4u URL"),
        })
    }

    pub fn with_base_url(base: &str) -> Result<Self, HdHub4uError> {
        let base_url = Url::parse(base).map_err(|_| HdHub4uError::InvalidUrl(base.to_string()))?;
        if base_url.scheme() != "https" {
            return Err(HdHub4uError::InvalidUrl(base.to_string()));
        }
        Ok(Self {
            client: build_client(),
            base_url,
        })
    }

    pub async fn health_check(&self) -> Result<(), HdHub4uError> {
        self.fetch_first_ok("").await.map(|_| ())
    }

    /// Search by scanning the WordPress post sitemaps for URLs whose slug
    /// contains every token of the query. The hdhub4u `?s=` endpoint redirects
    /// to a JS-rendered SPA that a plain HTTP client cannot execute, so the
    /// sitemap is the only server-rendered search surface available.
    pub async fn search(&self, query: &str) -> Result<Vec<CatalogItem>, HdHub4uError> {
        let tokens = tokenize_query(query);
        if tokens.is_empty() {
            // No query: return the homepage listing (latest releases).
            let (base, html) = self.fetch_first_ok("").await?;
            return parser::parse_search(&base, &html);
        }

        // Prefer the site's own `?s=` search — it returns full titles, posters,
        // and media types, and covers content missing from the sitemap.
        if let Ok((base, html)) = self
            .fetch_first_ok(&format!("?s={}", urlencode_query(query)))
            .await
        {
            if let Ok(items) = parser::parse_site_search(&base, &html) {
                if !items.is_empty() {
                    return Ok(items);
                }
            }
        }

        // Fallback: scan the post sitemaps for slugs containing every token.
        let mut urls = self.fetch_post_sitemap_urls().await?;
        urls.retain(|u| {
            let slug = u.trim_start_matches('/').trim_end_matches('/');
            let lower = slug.to_ascii_lowercase();
            tokens.iter().all(|tok| lower.contains(tok))
        });
        urls.truncate(40);

        let client = self.client.clone();
        let items: Vec<CatalogItem> = urls
            .into_iter()
            .filter_map(|full_url| {
                let parsed = Url::parse(&full_url).ok()?;
                let slug = parsed.path().trim_matches('/').to_string();
                if slug.is_empty() {
                    return None;
                }
                let title = slug_to_title(&slug);
                let media_type = parser::classify_media_type_pub(&slug, &title);
                let year = first_four_digit_year(&title);
                Some(CatalogItem {
                    id: ProviderMediaId {
                        provider: ProviderKind::HdHub4u,
                        value: format!("/{slug}/"),
                    },
                    title,
                    media_type,
                    year,
                    poster_url: None,
                    season_count: None,
                })
            })
            .collect();

        // Fetch poster images concurrently by grabbing og:image from each post page.
        let poster_fetches: Vec<_> = items
            .iter()
            .take(20)
            .map(|item| {
                let client = client.clone();
                let url = self
                    .provider_url(&item.id.value)
                    .unwrap_or(self.base_url.clone());
                async move {
                    let resp = client.get(url).send().await.ok()?;
                    let html = resp.text().await.ok()?;
                    let document = scraper::Html::parse_document(&html);
                    parser::extract_og_image(&document)
                }
            })
            .collect();
        let poster_results = futures::future::join_all(poster_fetches).await;

        let mut items = items;
        for (i, poster) in poster_results.into_iter().enumerate() {
            if let Some(p) = poster {
                items[i].poster_url = Some(p);
            }
        }

        if items.is_empty() {
            Err(HdHub4uError::Parse("no search results found".into()))
        } else {
            Ok(items)
        }
    }

    async fn fetch_post_sitemap_urls(&self) -> Result<Vec<String>, HdHub4uError> {
        // Try candidate bases: the sitemap index may only exist on the live mirror.
        let (_base, index_xml) = self.fetch_first_ok("sitemap.xml").await?;

        // Old WordPress SEO plugin: post-sitemap1.xml, post-sitemap2.xml, ...
        let mut post_sitemap_urls = extract_sitemap_locs(&index_xml, "post-sitemap");
        // Newer WordPress core: wp-sitemap-posts-movies-1.xml (movies), wp-sitemap-posts-*.xml
        if post_sitemap_urls.is_empty() {
            post_sitemap_urls = extract_sitemap_locs(&index_xml, "wp-sitemap-posts");
        }

        // Fetch all sitemap pages concurrently for speed.
        let client = self.client.clone();
        let fetches: Vec<_> = post_sitemap_urls
            .iter()
            .take(20)
            .filter_map(|sm_url| {
                let url = Url::parse(sm_url).ok()?;
                let client = client.clone();
                Some(async move {
                    let resp = client.get(url).send().await.ok()?;
                    let text = resp.text().await.ok()?;
                    Some(extract_locs(&text))
                })
            })
            .collect();
        let results = futures::future::join_all(fetches).await;
        let mut all = Vec::new();
        for urls in results.into_iter().flatten() {
            all.extend(urls);
        }
        Ok(all)
    }

    pub async fn details(&self, id: &str) -> Result<MediaDetails, HdHub4uError> {
        let (_base, html) = self.fetch_first_ok(id).await?;
        parser::parse_details(id, &html)
    }

    pub async fn releases(
        &self,
        id: &str,
        season: usize,
        episode: usize,
    ) -> Result<Vec<Release>, HdHub4uError> {
        let (_base, html) = self.fetch_first_ok(id).await?;
        parser::parse_releases(&html, season, episode)
    }

    pub async fn resolve_release(&self, release: &Release) -> Result<PlaybackSource, HdHub4uError> {
        if release.provider != ProviderKind::HdHub4u {
            return Err(HdHub4uError::Parse(
                "release belongs to another provider".into(),
            ));
        }
        for mirror in &release.mirrors {
            let candidates = resolve_mirror(&self.client, &mirror.resolver_url).await;
            if let Ok(candidates) = candidates {
                for (url, label, headers) in candidates {
                    if let Ok(playable_url) = self.preflight(&url, &headers).await {
                        return Ok(PlaybackSource {
                            provider: ProviderKind::HdHub4u,
                            url: playable_url,
                            headers,
                            subtitle: None,
                            source_label: label,
                        });
                    }
                }
            }
        }
        Err(HdHub4uError::NoPlayableMirror)
    }

    async fn preflight(
        &self,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<String, HdHub4uError> {
        hubcloud::validate_playback_url(url)
            .map_err(|e| HdHub4uError::InvalidUrl(e.to_string()))?;
        let mut request = self
            .client
            .get(url)
            .header(reqwest::header::RANGE, "bytes=0-0");
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await?.error_for_status()?;
        let final_url = response.url().clone();
        hubcloud::validate_playback_url(final_url.as_str())
            .map_err(|e| HdHub4uError::InvalidUrl(e.to_string()))?;
        Ok(final_url.to_string())
    }

    async fn fetch_text(&self, url: Url) -> Result<String, HdHub4uError> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        Ok(response.text().await?)
    }

    /// Candidate base URLs to try in order: current base first, then the
    /// known fallback mirrors. The site rotates domains frequently.
    fn candidate_bases(&self) -> Vec<Url> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let mut push = |raw: &str| {
            if let Ok(url) = Url::parse(raw) {
                if seen.insert(url.as_str().to_string()) {
                    out.push(url);
                }
            }
        };
        push(self.base_url.as_str());
        for candidate in CANDIDATE_BASE_URLS {
            push(candidate);
        }
        out
    }

    /// Fetch `path` from the first base that answers. Returns the working
    /// base URL and the response body so callers can reuse the right host.
    async fn fetch_first_ok(&self, path: &str) -> Result<(Url, String), HdHub4uError> {
        let mut last_err: Option<HdHub4uError> = None;
        for base in self.candidate_bases() {
            let url = match base.join(path.trim_start_matches('/')) {
                Ok(url) => url,
                Err(_) => continue,
            };
            match self.fetch_text(url).await {
                Ok(body) => return Ok((base, body)),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err
            .unwrap_or_else(|| HdHub4uError::Parse("all hdhub4u domains unreachable".into())))
    }

    fn provider_url(&self, id: &str) -> Result<Url, HdHub4uError> {
        let url = self
            .base_url
            .join(id.trim_start_matches('/'))
            .map_err(|_| HdHub4uError::InvalidUrl(id.to_string()))?;
        if url.host_str() != self.base_url.host_str() {
            return Err(HdHub4uError::InvalidUrl(id.to_string()));
        }
        Ok(url)
    }
}

/// Resolve a hdhub4u mirror URL through the host chain to direct media URLs.
///
/// Supported chains:
/// - `hubdrive.tips/file/{id}` → `hubcloud.cx/drive/{id}` → `gamerxyt.com/hubcloud.php` → direct
/// - `hubcloud.*/drive/{id}` → `gamerxyt.com/hubcloud.php` → direct (shared with 4khdhub)
/// - `hdstream4u.com/file/{id}` → `morencius.com/download/{id}` → direct
async fn resolve_mirror(
    client: &reqwest::Client,
    resolver_url: &str,
) -> Result<Vec<(String, String, Vec<(String, String)>)>, HdHub4uError> {
    let parsed =
        Url::parse(resolver_url).map_err(|_| HdHub4uError::InvalidUrl(resolver_url.into()))?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();

    if host.starts_with("hubdrive.") {
        let html = client
            .get(resolver_url)
            .send()
            .await
            .map_err(HdHub4uError::from)?
            .error_for_status()
            .map_err(HdHub4uError::from)?
            .text()
            .await
            .map_err(HdHub4uError::from)?;
        let hubcloud_url = parser::extract_hubcloud_drive_url(&html)
            .ok_or_else(|| HdHub4uError::Parse("HubDrive HubCloud mirror missing".into()))?;
        return hubcloud::resolve(client, &hubcloud_url)
            .await
            .map_err(|e| HdHub4uError::Parse(e.to_string()));
    }

    if host.starts_with("hubcloud.") && parsed.path().starts_with("/drive/") {
        return hubcloud::resolve(client, resolver_url)
            .await
            .map_err(|e| HdHub4uError::Parse(e.to_string()));
    }

    if host == "hdstream4u.com" {
        return resolve_hdstream4u(client, resolver_url).await;
    }

    // New hdhub4u.website chain: `daday.ejuda.online/movz/hubcloud/stream.php?id=...`
    // page contains `var originalLink = "https://pixeldrain.tech/u/{id}";`
    if host.ends_with("ejuda.online") {
        return resolve_ejuda_stream(client, resolver_url).await;
    }

    // Unknown host: do not fall back to direct validation — hosts like
    // hubcdn.sbs return "File Deleted" HTML and gadgetsweb.xyz redirects to
    // ad gates, neither of which is playable media.
    Err(HdHub4uError::Parse(format!(
        "unsupported resolver host: {host}"
    )))
}

/// ejuda.online stream.php page embeds `var originalLink = "...";` pointing at
/// a Pixeldrain-style share page. Convert it to the direct download URL.
async fn resolve_ejuda_stream(
    client: &reqwest::Client,
    stream_url: &str,
) -> Result<Vec<(String, String, Vec<(String, String)>)>, HdHub4uError> {
    let response = client
        .get(stream_url)
        .send()
        .await
        .map_err(HdHub4uError::from)?
        .error_for_status()
        .map_err(HdHub4uError::from)?;

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Some ejuda endpoints (e.g. /vcloudz/) return the raw video bytes directly.
    if !content_type.contains("html") {
        let direct_url = response.url().to_string();
        return Ok(vec![(direct_url, "Direct".to_string(), Vec::new())]);
    }

    let html = response.text().await.map_err(HdHub4uError::from)?;

    // var originalLink = "https://pixeldrain.tech/u/2iS8NY7s";
    let link = html
        .split("originalLink")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
        .map(str::trim)
        .filter(|s| s.starts_with("http"))
        .ok_or_else(|| HdHub4uError::Parse("ejuda stream originalLink missing".into()))?;

    let parsed = Url::parse(link).map_err(|_| HdHub4uError::InvalidUrl(link.into()))?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();

    // Pixeldrain share page → direct download API URL.
    if host.contains("pixeldrain") && parsed.path().starts_with("/u/") {
        let file_id = parsed
            .path()
            .trim_start_matches("/u/")
            .trim_end_matches('/');
        if !file_id.is_empty() {
            let direct = format!("https://pixeldrain.dev/api/file/{file_id}?download");
            return Ok(vec![(direct, "PixelDrain".to_string(), Vec::new())]);
        }
    }

    // Fall back to the raw link; preflight() will validate it.
    Ok(vec![(
        link.to_string(),
        "PixelDrain".to_string(),
        Vec::new(),
    )])
}

/// hdstream4u.com/file/{id} embeds a `morencius.com/download/{id}` link whose page
/// exposes the direct media download (`/download/{id}_n` for HD, `/download/{id}_l` for SD).
async fn resolve_hdstream4u(
    client: &reqwest::Client,
    file_url: &str,
) -> Result<Vec<(String, String, Vec<(String, String)>)>, HdHub4uError> {
    let parsed = Url::parse(file_url).map_err(|_| HdHub4uError::InvalidUrl(file_url.into()))?;
    let Some(file_id) = parsed
        .path()
        .strip_prefix("/file/")
        .map(|s| s.trim_matches('/'))
        .filter(|s| !s.is_empty())
    else {
        return Err(HdHub4uError::Parse("hdstream4u file id missing".into()));
    };

    let embed_html = client
        .get(file_url)
        .send()
        .await
        .map_err(HdHub4uError::from)?
        .error_for_status()
        .map_err(HdHub4uError::from)?
        .text()
        .await
        .map_err(HdHub4uError::from)?;

    let morencius_base = parser::extract_morencius_download_url(&embed_html)
        .ok_or_else(|| HdHub4uError::Parse("morencius download link missing".into()))?;

    let dl_html = client
        .get(&morencius_base)
        .send()
        .await
        .map_err(HdHub4uError::from)?
        .error_for_status()
        .map_err(HdHub4uError::from)?
        .text()
        .await
        .map_err(HdHub4uError::from)?;

    let mut candidates = Vec::new();
    for (suffix, label) in [("_n", "HD"), ("_l", "SD")] {
        let url = format!("{morencius_base}{suffix}");
        if hubcloud::validate_playback_url(&url).is_ok() && dl_html.contains(&url) {
            candidates.push((url, format!("EarnVids {label}"), Vec::new()));
        }
    }
    // Fallback: derive the download endpoints from the file id directly.
    if candidates.is_empty() {
        for (suffix, label) in [("_n", "HD"), ("_l", "SD")] {
            let url = format!("https://morencius.com/download/{file_id}{suffix}");
            if hubcloud::validate_playback_url(&url).is_ok() {
                candidates.push((url, format!("EarnVids {label}"), Vec::new()));
            }
        }
    }
    if candidates.is_empty() {
        Err(HdHub4uError::NoPlayableMirror)
    } else {
        Ok(candidates)
    }
}

/// Percent-encode a search query for use in a URL query string.
fn urlencode_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Lowercase, dedupe, drop common stopwords and short tokens from a search query.
fn tokenize_query(query: &str) -> Vec<String> {
    query
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .filter(|t| {
            !matches!(
                *t,
                "the" | "a" | "an" | "of" | "and" | "in" | "on" | "at" | "to"
            )
        })
        .map(|t| t.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Convert a URL slug like "the-devils-mouth-2026-hindi-webrip-full-movie"
/// into a human-readable title "The Devils Mouth 2026 Hindi Webrip Full Movie".
fn slug_to_title(slug: &str) -> String {
    // Newer hdhub4u.website URLs are /watch/{slug}/ — strip the prefix.
    let slug = slug.trim_start_matches("watch/").trim_start_matches('/');
    slug.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pull `<loc>` URLs from a sitemap XML that contain `name_filter`.
fn extract_sitemap_locs(xml: &str, name_filter: &str) -> Vec<String> {
    let mut out = Vec::new();
    for window in xml.match_indices("<loc>").map(|(i, _)| i) {
        let start = window + 5;
        if let Some(end) = xml[start..].find("</loc>") {
            let loc = xml[start..start + end].trim();
            if loc.contains(name_filter) {
                out.push(loc.to_string());
            }
        }
    }
    out
}

/// Pull all `<loc>` URLs from a sitemap XML.
fn extract_locs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    for window in xml.match_indices("<loc>").map(|(i, _)| i) {
        let start = window + 5;
        if let Some(end) = xml[start..].find("</loc>") {
            let loc = xml[start..start + end].trim();
            // post URLs have at least two path segments: /slug/
            if loc.matches('/').count() >= 4 {
                out.push(loc.to_string());
            }
        }
    }
    out
}

/// First 4-digit year (1930-2100) in a string.
fn first_four_digit_year(value: &str) -> Option<String> {
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut buf = String::from(c);
            for d in chars.by_ref().take(3) {
                if d.is_ascii_digit() {
                    buf.push(d);
                } else {
                    break;
                }
            }
            if buf.len() == 4 {
                let year: u32 = buf.parse().ok()?;
                if (1930..=2100).contains(&year) {
                    return Some(buf);
                }
            }
        }
    }
    None
}

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(5))
        .user_agent("Mozilla/5.0 HDHub4u-TUI/0.1")
        .build()
        .unwrap_or_default()
}
