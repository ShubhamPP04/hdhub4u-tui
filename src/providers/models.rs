use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    MovieBox,
    FourKHdHub,
    HdHub4u,
}

impl ProviderKind {
    pub const ENABLED: [Self; 3] = [Self::MovieBox, Self::FourKHdHub, Self::HdHub4u];

    pub const fn cache_key(self) -> &'static str {
        match self {
            Self::MovieBox => "moviebox",
            Self::FourKHdHub => "fourkhdhub",
            Self::HdHub4u => "hdhub4u",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::MovieBox => "MovieBox",
            Self::FourKHdHub => "4KHDHub",
            Self::HdHub4u => "HDHub4u",
        }
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderMediaId {
    pub provider: ProviderKind,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestContext {
    pub provider: ProviderKind,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Movie,
    Series,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: ProviderMediaId,
    pub title: String,
    pub media_type: MediaType,
    pub year: Option<String>,
    pub poster_url: Option<String>,
    pub season_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub season: usize,
    pub number: usize,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Season {
    pub number: usize,
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaDetails {
    pub id: ProviderMediaId,
    pub title: String,
    pub media_type: MediaType,
    pub year: Option<String>,
    pub description: Option<String>,
    pub tagline: Option<String>,
    pub imdb_rating: Option<String>,
    pub director: Option<String>,
    pub stars: Option<String>,
    pub prints: Option<String>,
    pub audios: Option<String>,
    pub poster_url: Option<String>,
    pub genres: Vec<String>,
    pub seasons: Vec<Season>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMirror {
    pub label: String,
    pub resolver_url: String,
    pub headers: Vec<(String, String)>,
    pub direct_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub provider: ProviderKind,
    pub filename: String,
    pub quality: Option<String>,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub size_bytes: Option<u64>,
    pub season: Option<usize>,
    pub episode: Option<usize>,
    pub mirrors: Vec<SourceMirror>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackSource {
    pub provider: ProviderKind,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub subtitle: Option<String>,
    pub source_label: String,
}

impl PlaybackSource {
    pub fn bare(provider: ProviderKind, url: impl Into<String>, subtitle: Option<String>) -> Self {
        Self {
            provider,
            url: url.into(),
            headers: Vec::new(),
            subtitle,
            source_label: provider.label().to_string(),
        }
    }
}
