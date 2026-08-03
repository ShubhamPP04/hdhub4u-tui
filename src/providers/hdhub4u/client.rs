use super::parser;
use crate::providers::fourkhdhub::hubcloud;
use crate::providers::models::{
    CatalogItem, MediaDetails, PlaybackSource, ProviderKind, ProviderMediaId, Release,
};
use reqwest::Url;

const DEFAULT_BASE_URL: &str = "https://new4.hdhub4u.cl/";

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
        let base = std::env::var("HDHUB4U_HDHUB4U_URL")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::with_base_url(&base).unwrap_or_else(|_| Self {
            client: build_client(),
            base_url: Url::parse(DEFAULT_BASE_URL).expect("valid default HDHub4u URL"),
        })
    }

    pub fn with_base_url(base: &str) -> Result<Self, HdHub4uError> {
        let base_url =
            Url::parse(base).map_err(|_| HdHub4uError::InvalidUrl(base.to_string()))?;
        if base_url.scheme() != "https" {
            return Err(HdHub4uError::InvalidUrl(base.to_string()));
        }
        Ok(Self {
            client: build_client(),
            base_url,
        })
    }

    pub async fn health_check(&self) -> Result<(), HdHub4uError> {
        let response = self.client.get(self.base_url.clone()).send().await?;
        if !response.status().is_success() {
            return Err(HdHub4uError::Parse(format!(
                "health check returned {}",
                response.status()
            )));
        }
        Ok(())
    }

    /// Search by scanning the WordPress post sitemaps for URLs whose slug
    /// contains every token of the query. The hdhub4u `?s=` endpoint redirects
    /// to a JS-rendered SPA that a plain HTTP client cannot execute, so the
    /// sitemap is the only server-rendered search surface available.
    pub async fn search(&self, query: &str) -> Result<Vec<CatalogItem>, HdHub4uError> {
        let tokens = tokenize_query(query);
        if tokens.is_empty() {
            // No query: return the homepage listing (latest releases).
            let html = self.fetch_text(self.base_url.clone()).await?;
            return parser::parse_search(&self.base_url, &html);
        }

        let mut urls = self.fetch_post_sitemap_urls().await?;
        urls.retain(|u| {
            let slug = u
                .trim_start_matches('/')
                .trim_end_matches('/');
            let lower = slug.to_ascii_lowercase();
            tokens.iter().all(|tok| lower.contains(tok))
        });
        urls.truncate(40);

        let items: Vec<CatalogItem> = urls
            .into_iter()
            .filter_map(|full_url| {
                let parsed = Url::parse(&full_url).ok()?;
                // Extract just the path (slug) from the full sitemap URL.
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
        if items.is_empty() {
            Err(HdHub4uError::Parse("no search results found".into()))
        } else {
            Ok(items)
        }
    }

    async fn fetch_post_sitemap_urls(&self) -> Result<Vec<String>, HdHub4uError> {
        let index_url = self.base_url.join("sitemap.xml").unwrap_or(self.base_url.clone());
        let index_xml = self.fetch_text(index_url).await?;
        let post_sitemap_urls = extract_sitemap_locs(&index_xml, "post-sitemap");
        let mut all = Vec::new();
        for sm_url in post_sitemap_urls.iter().take(20) {
            if let Ok(url) = Url::parse(sm_url) {
                if let Ok(xml) = self.fetch_text(url).await {
                    all.extend(extract_locs(&xml));
                }
            }
        }
        Ok(all)
    }

    pub async fn details(&self, id: &str) -> Result<MediaDetails, HdHub4uError> {
        let url = self.provider_url(id)?;
        let html = self.fetch_text(url).await?;
        parser::parse_details(id, &html)
    }

    pub async fn releases(
        &self,
        id: &str,
        season: usize,
        episode: usize,
    ) -> Result<Vec<Release>, HdHub4uError> {
        let url = self.provider_url(id)?;
        let html = self.fetch_text(url).await?;
        parser::parse_releases(&html, season, episode)
    }

    pub async fn resolve_release(
        &self,
        release: &Release,
    ) -> Result<PlaybackSource, HdHub4uError> {
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
    let parsed = Url::parse(resolver_url).map_err(|_| HdHub4uError::InvalidUrl(resolver_url.into()))?;
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

    // Unknown host: do not fall back to direct validation — hosts like
    // hubcdn.sbs return "File Deleted" HTML and gadgetsweb.xyz redirects to
    // ad gates, neither of which is playable media.
    Err(HdHub4uError::Parse(format!(
        "unsupported resolver host: {host}"
    )))
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
        if hubcloud::validate_playback_url(&url).is_ok()
            && dl_html.contains(&url)
        {
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

/// Lowercase, dedupe, drop common stopwords and short tokens from a search query.
fn tokenize_query(query: &str) -> Vec<String> {
    query
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .filter(|t| !matches!(*t, "the" | "a" | "an" | "of" | "and" | "in" | "on" | "at" | "to"))
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
        .user_agent("Mozilla/5.0 MovieBox-TUI/0.1")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default()
}
