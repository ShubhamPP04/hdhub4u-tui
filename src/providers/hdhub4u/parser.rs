use super::client::HdHub4uError;
use crate::providers::models::{
    CatalogItem, Episode, MediaDetails, MediaType, ProviderKind, ProviderMediaId, Release, Season,
    SourceMirror,
};
use reqwest::Url;
use scraper::{ElementRef, Html, Selector};
use std::collections::BTreeMap;

/// Hosts that serve the actual media file or a resolver page we can follow.
const DOWNLOAD_HOSTS: &[&str] = &[
    "hubcdn.sbs",
    "hubdrive.tips",
    "gadgetsweb.xyz",
    "hdstream4u.com",
    "hubstream.art",
    "hubcloud.",
];

pub fn parse_search(base: &Url, html: &str) -> Result<Vec<CatalogItem>, HdHub4uError> {
    let document = Html::parse_document(html);
    // Homepage/category listing: <li class="thumb ..."><figure><img title="..."/><a href=".../slug/">...</a></figure><figcaption><a href=".../slug/"><p>TITLE</p></a></figcaption></li>
    let item = selector("li.thumb")?;
    let link = selector("a[href]")?;
    let title_p = selector("p")?;
    let img = selector("img")?;

    let base_host = base.host_str().unwrap_or_default();
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for node in document.select(&item) {
        // The figcaption <a> holds the canonical post URL + title text.
        let mut post_url: Option<String> = None;
        let mut title: Option<String> = None;
        let mut poster: Option<String> = None;

        for a in node.select(&link) {
            if let Some(href) = a.value().attr("href") {
                if let Ok(url) = Url::parse(href) {
                    if url.host_str() == Some(base_host) && url.path().matches('/').count() >= 2 {
                        // post URL like https://host/slug/
                        post_url = Some(url.path().to_string());
                        // title is the <p> inside this <a>
                        if let Some(p) = a.select(&title_p).next() {
                            let t = p.text().collect::<Vec<_>>().join("").trim().to_string();
                            if !t.is_empty() {
                                title = Some(t);
                            }
                        }
                    }
                }
            }
        }
        if let Some(img_node) = node.select(&img).next() {
            poster = img_node
                .value()
                .attr("src")
                .map(|s| s.to_string())
                .or_else(|| img_node.value().attr("data-src").map(|s| s.to_string()));
            if title.is_none() {
                title = img_node.value().attr("title").map(|s| s.to_string());
            }
        }

        if let (Some(id), Some(t)) = (post_url, title) {
            let key = id.clone();
            if seen.insert(key) {
                let media_type = classify_media_type(&id, &t);
                let year = first_four_digit_year(&t);
                let season_count = if media_type == MediaType::Series {
                    detect_season_count(&id, &t)
                } else {
                    None
                };
                items.push(CatalogItem {
                    id: ProviderMediaId {
                        provider: ProviderKind::HdHub4u,
                        value: id,
                    },
                    title: strip_trailing_year(&t),
                    media_type,
                    year,
                    poster_url: poster,
                    season_count,
                });
            }
        }
    }

    if items.is_empty() {
        return Err(HdHub4uError::Parse("no search results found".into()));
    }
    Ok(items)
}

pub fn parse_details(id: &str, html: &str) -> Result<MediaDetails, HdHub4uError> {
    let document = Html::parse_document(html);
    let h1 = selector("h1.page-title span.material-text, h1 span, h1")?;
    let raw_title = document
        .select(&h1)
        .next()
        .map(|e| {
            e.text()
                .collect::<String>()
                .trim()
                .trim_start_matches(|c: char| !c.is_alphanumeric())
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .ok_or_else(|| HdHub4uError::Parse("title missing".into()))?;
    let title = strip_trailing_year(&raw_title);
    let media_type = classify_media_type(id, &raw_title);

    let genres = extract_genres(&document);
    let (imdb_rating, stars, director, language, quality, episode_count) =
        extract_meta_block(&document);
    let poster = extract_og_image(&document);
    let description = extract_storyline(&document);
    let seasons = parse_seasons(&document, media_type, episode_count);

    Ok(MediaDetails {
        id: ProviderMediaId {
            provider: ProviderKind::HdHub4u,
            value: id.to_string(),
        },
        title,
        media_type,
        year: first_four_digit_year(&raw_title),
        description,
        tagline: None,
        imdb_rating,
        director,
        stars,
        prints: quality,
        audios: language,
        poster_url: poster,
        genres,
        seasons,
    })
}

pub fn parse_releases(
    html: &str,
    season: usize,
    episode: usize,
) -> Result<Vec<Release>, HdHub4uError> {
    let document = Html::parse_document(html);
    let a = selector("a[href]")?;

    let mut grouped: BTreeMap<String, Release> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();

    for node in document.select(&a) {
        let Some(href) = node.value().attr("href") else {
            continue;
        };
        let Ok(url) = Url::parse(href) else {
            continue;
        };
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if !DOWNLOAD_HOSTS.iter().any(|h| host.contains(h)) {
            continue;
        }
        // Skip the nav "4K Movies" link to 4khdhub and cross-links to other posts.
        if host == "4khdhub.one" {
            continue;
        }
        let label = node.text().collect::<Vec<_>>().join("").trim().to_string();
        if label.is_empty() {
            continue;
        }
        // For series pages, episode links are labelled "EPiSODE N". Parse the
        // episode number from the label; for movies the label carries quality+size.
        let (ep_num, rel_filename, rel_season) = if let Some(n) = parse_episode_label(&label) {
            let fname = format!(
                "{} S{:02}E{:02}",
                "Episode",
                if season > 0 { season } else { 1 },
                n
            );
            (Some(n), fname, Some(if season > 0 { season } else { 1 }))
        } else {
            // Movie release: use the label as the filename.
            (None, label.clone(), None)
        };

        // When a specific episode is requested, skip links for other episodes.
        if season > 0 && episode > 0 {
            if let Some(n) = ep_num {
                if n != episode {
                    continue;
                }
            }
        }

        let mirror = SourceMirror {
            label: clean_label(&label),
            resolver_url: url.to_string(),
            headers: Vec::new(),
            direct_file: false,
        };

        let key = if let Some(n) = ep_num {
            format!(
                "s{:02}e{:02}",
                if season > 0 { season } else { 1 },
                n
            )
        } else {
            // Movie: each quality/size link is a separate release, not a mirror.
            label.clone()
        };

        let entry = grouped.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Release {
                provider: ProviderKind::HdHub4u,
                filename: rel_filename,
                quality: detect_quality(&label),
                codec: detect_codec(&label),
                language: detect_language(&label),
                size_bytes: parse_size_bytes(&label),
                season: rel_season,
                episode: ep_num,
                mirrors: Vec::new(),
            }
        });
        entry.mirrors.push(mirror);
    }

    let releases: Vec<Release> = order
        .into_iter()
        .filter_map(|k| grouped.remove(&k))
        .collect();

    if releases.is_empty() {
        return Err(HdHub4uError::Parse("no download links found".into()));
    }
    Ok(releases)
}

/// Extract a `hubcloud.*/drive/...` URL from a hubdrive.tips page.
pub fn extract_hubcloud_drive_url(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let links = Selector::parse("a[href]").ok()?;
    document.select(&links).find_map(|node| {
        let raw = node.value().attr("href")?;
        let url = Url::parse(raw).ok()?;
        let host = url.host_str()?;
        (host.starts_with("hubcloud.") && url.path().starts_with("/drive/")).then(|| url.to_string())
    })
}

/// Extract the `morencius.com/download/{id}` URL from an hdstream4u embed page.
pub fn extract_morencius_download_url(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let links = Selector::parse("a[href]").ok()?;
    document.select(&links).find_map(|node| {
        let raw = node.value().attr("href")?;
        let url = Url::parse(raw).ok()?;
        let host = url.host_str()?;
        (host == "morencius.com" && url.path().starts_with("/download/")).then(|| url.to_string())
    })
}

fn extract_meta_block(
    document: &Html,
) -> (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<usize>) {
    let strong = selector("strong").unwrap();
    let mut imdb: Option<String> = None;
    let mut stars: Option<String> = None;
    let mut director: Option<String> = None;
    let mut language: Option<String> = None;
    let mut quality: Option<String> = None;
    let mut episode_count: Option<usize> = None;

    for node in document.select(&strong) {
        let text = node.text().collect::<String>().trim().to_lowercase();
        let sibling_text = node
            .next_sibling()
            .and_then(|s| ElementRef::wrap(s))
            .map(|e| e.text().collect::<String>().trim().to_string())
            .or_else(|| {
                // value may be inside an <a> following the <strong>
                node.parent()
                    .and_then(ElementRef::wrap)
                    .map(|p| {
                        let full = p.text().collect::<String>();
                        // strip the label prefix
                        full.split(':').nth(1).map(|s| s.trim().to_string())
                    })
                    .flatten()
            });
        let value = sibling_text;
        match text.trim_end_matches(':') {
            "imdb rating" | "imdb" | "rating" => {
                if imdb.is_none() {
                    imdb = value.or_else(|| extract_imdb_link(document));
                }
            }
            "stars" => stars = value.or(stars),
            "director" | "creator" | "creators" => director = value.or(director),
            "language" | "languages" => language = value.or(language),
            "quality" => quality = value.or(quality),
            "no. of episodes" => {
                if episode_count.is_none() {
                    episode_count = value.and_then(|v| parse_season_count(&v));
                }
            }
            _ => {}
        }
    }
    // Fallback: scan the full document text for "iMDB Rating: X.X/10".
    if imdb.is_none() {
        imdb = extract_imdb_rating_from_text(document);
    }
    if episode_count.is_none() {
        episode_count = extract_episode_count_from_text(document);
    }
    (imdb, stars, director, language, quality, episode_count)
}

fn extract_imdb_link(document: &Html) -> Option<String> {
    let a = selector("a[href*='imdb.com']").unwrap();
    document
        .select(&a)
        .next()
        .and_then(|n| {
            let href = n.value().attr("href")?;
            // Only consider imdb title links (tt-prefixed ids).
            if !href.contains("/title/tt") {
                return None;
            }
            Some(n.text().collect::<String>().trim().to_string())
        })
        .filter(|s| !s.is_empty())
}

fn extract_imdb_rating_from_text(document: &Html) -> Option<String> {
    let full = document.root_element().text().collect::<String>();
    let idx = full.to_ascii_lowercase().find("imdb rating")?;
    let tail = &full[idx..];
    // find a number like 8.0/10
    let rest = tail.split(':').nth(1)?;
    let rating = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '/')
        .collect::<String>();
    if rating.contains('/') {
        Some(rating)
    } else {
        None
    }
}

fn extract_episode_count_from_text(document: &Html) -> Option<usize> {
    let full = document.root_element().text().collect::<String>();
    let idx = full.to_ascii_lowercase().find("no. of episodes")?;
    let tail = &full[idx..];
    let rest = tail.split(':').nth(1)?;
    // first run of digits
    let digits = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn extract_genres(document: &Html) -> Vec<String> {
    let mut genres = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Primary source: the "Genre: A | B | C" metadata line in the post body.
    if let Some(line) = find_meta_line(document, "Genre") {
        for g in line.split('|').map(str::trim) {
            if !g.is_empty() && seen.insert(g.to_string()) {
                genres.push(g.to_string());
            }
        }
    }

    // Fallback: category links in page-meta that are actual genre names.
    if genres.is_empty() {
        let a = selector("a[href*='/category/']").unwrap();
        for node in document.select(&a) {
            let text = node.text().collect::<String>().trim().to_string();
            if !text.is_empty() && seen.insert(text.clone()) && is_genre(&text) {
                genres.push(text);
            }
        }
    }
    genres
}

fn find_meta_line(document: &Html, label: &str) -> Option<String> {
    let strong = selector("strong").ok()?;
    for node in document.select(&strong) {
        let text = node.text().collect::<String>().trim().to_lowercase();
        if text.trim_end_matches(':') == label.to_ascii_lowercase() {
            // value is the text following this <strong> within its parent.
            if let Some(parent) = node.parent().and_then(ElementRef::wrap) {
                let full = parent.text().collect::<String>();
                return full.split(':').nth(1).map(|s| s.trim().to_string());
            }
        }
    }
    None
}

fn extract_og_image(document: &Html) -> Option<String> {
    let meta = selector("meta[property='og:image']").unwrap();
    document
        .select(&meta)
        .next()
        .and_then(|n| n.value().attr("content"))
        .map(|s| s.to_string())
}

fn extract_storyline(document: &Html) -> Option<String> {
    // Storyline appears under an h2/h3 "Storyline" heading, then a <p> with <em>.
    let full = document.root_element().text().collect::<String>();
    let idx = full.to_ascii_lowercase().find("storyline")?;
    let tail = &full[idx..];
    // take up to next "Review" or "Download" heading
    let end = tail
        .find("Review")
        .or_else(|| tail.find("Download "))
        .unwrap_or(tail.len().min(600));
    let story = tail[..end].trim().to_string();
    if story.is_empty() {
        None
    } else {
        Some(story)
    }
}

fn parse_seasons(
    document: &Html,
    media_type: MediaType,
    episode_count: Option<usize>,
) -> Vec<Season> {
    if media_type != MediaType::Series {
        return Vec::new();
    }
    // Detect season number from the title/id, default 1.
    let season = 1usize;
    let mut episodes: BTreeMap<usize, Episode> = BTreeMap::new();
    let a = selector("a[href]").unwrap();
    for node in document.select(&a) {
        let Some(href) = node.value().attr("href") else {
            continue;
        };
        let Ok(url) = Url::parse(href) else {
            continue;
        };
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if !DOWNLOAD_HOSTS.iter().any(|h| host.contains(h)) {
            continue;
        }
        let label = node.text().collect::<Vec<_>>().join("").trim().to_string();
        if let Some(n) = parse_episode_label(&label) {
            episodes.entry(n).or_insert(Episode {
                season,
                number: n,
                title: Some(format!("Episode {n}")),
            });
        }
    }
    if episodes.is_empty() {
        if let Some(count) = episode_count {
            for n in 1..=count {
                episodes.insert(
                    n,
                    Episode {
                        season,
                        number: n,
                        title: Some(format!("Episode {n}")),
                    },
                );
            }
        }
    }
    if episodes.is_empty() {
        return Vec::new();
    }
    vec![Season {
        number: season,
        episodes: episodes.into_values().collect(),
    }]
}

fn classify_media_type(id: &str, title: &str) -> MediaType {
    let hay = format!("{id} {title}").to_ascii_lowercase();
    if hay.contains("season")
        || hay.contains("web-series")
        || hay.contains("web series")
        || hay.contains("all-episodes")
        || hay.contains("all episodes")
        || hay.contains("-series-")
        || hay.contains("ep-")
        || hay.contains("episode")
        || hay.contains("nf series")
        || hay.contains("hbo series")
    {
        MediaType::Series
    } else {
        MediaType::Movie
    }
}

/// Public wrapper for the client's sitemap-based search.
pub fn classify_media_type_pub(id: &str, title: &str) -> MediaType {
    classify_media_type(id, title)
}

fn detect_season_count(id: &str, title: &str) -> Option<usize> {
    let hay = format!("{id} {title}").to_ascii_lowercase();
    // "season 3" or "s03"
    if let Some(idx) = hay.find("season ") {
        let rest = &hay[idx + 7..];
        let digits = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if let Ok(n) = digits.parse() {
            return Some(n);
        }
    }
    None
}

fn parse_episode_label(label: &str) -> Option<usize> {
    let lower = label.to_ascii_lowercase();
    let idx = lower.find("episode")?;
    let rest = &lower[idx + 7..];
    let digits = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn clean_label(label: &str) -> String {
    let clean = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        "Direct".into()
    } else {
        clean
    }
}

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

fn strip_trailing_year(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(y) = first_four_digit_year(trimmed) {
        if let Some(idx) = trimmed.rfind(&y) {
            let left = trimmed[..idx].trim_end_matches(|c: char| c.is_whitespace() || c == '(');
            if !left.is_empty() {
                return left.to_string();
            }
        }
    }
    trimmed.to_string()
}

fn parse_size_bytes(value: &str) -> Option<u64> {
    // Walk for a number followed by a unit (e.g. "1.8gb", "420mb").
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut number = String::new();
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                number.push(chars[i]);
                i += 1;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let unit_start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let unit: String = chars[unit_start..i].iter().collect();
            let unit = unit.to_ascii_lowercase();
            let n: f64 = number.parse().ok()?;
            let mult = match unit.as_str() {
                "gb" => 1_073_741_824.0,
                "mb" => 1_048_576.0,
                "kb" => 1024.0,
                _ => 0.0,
            };
            if mult > 0.0 {
                return Some((n * mult) as u64);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn detect_quality(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let candidates = [
        ("4k", "4K"),
        ("2160p", "2160p"),
        ("1080p", "1080p"),
        ("720p", "720p"),
        ("480p", "480p"),
        ("hdrip", "HDRip"),
        ("web-dl", "WEB-DL"),
        ("bluray", "BluRay"),
        ("hdtc", "HDTC"),
        ("dvdrip", "DVDRip"),
    ];
    for (needle, label) in candidates {
        if lower.contains(needle) {
            return Some(label.to_string());
        }
    }
    None
}

fn detect_codec(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("10bit") || lower.contains("hevc") || lower.contains("x265") {
        Some("HEVC 10Bit".to_string())
    } else if lower.contains("x264") || lower.contains("h264") {
        Some("x264".to_string())
    } else {
        None
    }
}

fn detect_language(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let lang_map = [
        ("hindi", "Hindi"),
        ("english", "English"),
        ("tamil", "Tamil"),
        ("telugu", "Telugu"),
        ("malayalam", "Malayalam"),
        ("punjabi", "Punjabi"),
        ("korean", "Korean"),
        ("japanese", "Japanese"),
    ];
    let mut found = Vec::new();
    for (needle, label) in lang_map {
        if lower.contains(needle) && !found.contains(&label.to_string()) {
            found.push(label.to_string());
        }
    }
    if found.is_empty() {
        None
    } else {
        Some(found.join(" + "))
    }
}

fn parse_season_count(value: &str) -> Option<usize> {
    let digits = value
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn is_genre(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    const GENRES: &[&str] = &[
        "action", "adventure", "animation", "biography", "comedy", "crime", "documentary",
        "drama", "family", "fantasy", "history", "horror", "music", "musical", "mystery",
        "romance", "romantic", "sci-fi", "science fiction", "sport", "thriller", "war",
        "adult", "classic",
    ];
    GENRES.iter().any(|g| lower == *g || lower.contains(g))
}

fn selector(value: &str) -> Result<Selector, HdHub4uError> {
    Selector::parse(value).map_err(|_| HdHub4uError::Parse(format!("invalid selector: {value}")))
}

// --- moviebox JSON shape converters (mirror the fourkhdhub ones) ---

pub fn search_to_moviebox_json(items: &[CatalogItem]) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = items
        .iter()
        .map(|item| {
            serde_json::json!({
                "id": item.id.value,
                "title": item.title,
                "year": item.year,
                "media_type": match item.media_type {
                    MediaType::Movie => "movie",
                    MediaType::Series => "series",
                },
                "poster_url": item.poster_url,
                "season_count": item.season_count,
            })
        })
        .collect();
    serde_json::json!({ "results": rows })
}

pub fn details_to_moviebox_json(details: &MediaDetails) -> serde_json::Value {
    let seasons: Vec<serde_json::Value> = details
        .seasons
        .iter()
        .map(|s| {
            let episodes: Vec<serde_json::Value> = s
                .episodes
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "season": e.season,
                        "number": e.number,
                        "title": e.title,
                    })
                })
                .collect();
            serde_json::json!({ "number": s.number, "episodes": episodes })
        })
        .collect();
    serde_json::json!({
        "id": details.id.value,
        "title": details.title,
        "year": details.year,
        "media_type": match details.media_type {
            MediaType::Movie => "movie",
            MediaType::Series => "series",
        },
        "description": details.description,
        "imdb_rating": details.imdb_rating,
        "director": details.director,
        "stars": details.stars,
        "prints": details.prints,
        "audios": details.audios,
        "poster_url": details.poster_url,
        "genres": details.genres,
        "seasons": seasons,
    })
}

pub fn releases_to_moviebox_json(releases: &[Release]) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = releases
        .iter()
        .map(|r| {
            let mirrors: Vec<serde_json::Value> = r
                .mirrors
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "label": m.label,
                        "resolver_url": m.resolver_url,
                        "direct_file": m.direct_file,
                    })
                })
                .collect();
            serde_json::json!({
                "filename": r.filename,
                "quality": r.quality,
                "codec": r.codec,
                "language": r.language,
                "size_bytes": r.size_bytes,
                "season": r.season,
                "episode": r.episode,
                "mirrors": mirrors,
            })
        })
        .collect();
    serde_json::json!({ "results": rows })
}

