const OWNER: &str = "mesamirh";
const REPOSITORY: &str = "MovieBox-Tui";

pub async fn check(current: &str) -> Result<Option<String>, String> {
    let release = fetch_release().await?;

    if !is_newer(current, &release.version) {
        return Ok(None);
    }

    Ok(Some(release.version.clone()))
}

fn is_newer(current: &str, other: &str) -> bool {
    let parse = |v: &str| semver::Version::parse(v.trim_start_matches('v'));
    match (parse(current), parse(other)) {
        (Ok(cur), Ok(o)) => o > cur,
        _ => other != current,
    }
}

async fn fetch_release() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{OWNER}/{REPOSITORY}/releases/latest");
    let client = http_client()?;

    let mut request = client.get(&url);
    if let Some(token) = std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty()) {
        request = request.header("Authorization", format!("Bearer {token}"));
    }

    let resp = request
        .send()
        .await
        .map_err(|e| format!("GitHub request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub API {status}: {body}"));
    }

    let item: serde_json::Value = resp.json().await.map_err(|e| format!("bad JSON: {e}"))?;
    let tag = item["tag_name"].as_str().ok_or("missing tag_name")?;
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
    })
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("MovieBox-Tui")
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))
}

struct Release {
    version: String,
}
