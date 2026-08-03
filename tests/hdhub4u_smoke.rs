use hdhub4u_tui::providers::hdhub4u::HdHub4uClient;

#[tokio::test]
#[ignore = "hits live network; run with --ignored"]
async fn hdhub4u_search_details_resolve() {
    let client = HdHub4uClient::new();

    // 1. Search
    println!("--- SEARCH: devil ---");
    let results = client.search("devil mouth").await;
    match &results {
        Ok(items) => {
            println!("search returned {} items", items.len());
            for it in items.iter().take(5) {
                println!(
                    "  id={} title={:?} type={:?} year={:?} poster={:?}",
                    it.id.value,
                    it.title,
                    it.media_type,
                    it.year,
                    it.poster_url.is_some()
                );
            }
        }
        Err(e) => println!("search error: {e:?}"),
    }
    let items = results.expect("search should return results");
    assert!(!items.is_empty(), "search should find at least one result");

    // Pick the first result
    let item = &items[0];
    println!("\n--- DETAILS for {} ---", item.id.value);
    let details = client.details(&item.id.value).await;
    match &details {
        Ok(d) => {
            println!("title: {}", d.title);
            println!("year: {:?}", d.year);
            println!("media_type: {:?}", d.media_type);
            println!("imdb: {:?}", d.imdb_rating);
            println!("director: {:?}", d.director);
            println!("stars: {:?}", d.stars);
            println!("genres: {:?}", d.genres);
            println!("poster: {:?}", d.poster_url);
            println!("seasons: {}", d.seasons.len());
        }
        Err(e) => println!("details error: {e:?}"),
    }
    let d = details.expect("details should parse");

    // 2. Releases (season 0, episode 0 for movies; 1,1 for series)
    let (s, e) = if d.media_type == hdhub4u_tui::providers::models::MediaType::Series {
        (1, 1)
    } else {
        (0, 0)
    };
    println!("\n--- RELEASES (s={s} e={e}) ---");
    let releases = client.releases(&item.id.value, s, e).await;
    match &releases {
        Ok(rs) => {
            println!("releases: {}", rs.len());
            for r in rs.iter() {
                println!(
                    "  filename={:?} quality={:?} size={:?} mirrors={}",
                    r.filename,
                    r.quality,
                    r.size_bytes,
                    r.mirrors.len()
                );
                for m in r.mirrors.iter().take(3) {
                    println!("    mirror: {} -> {}", m.label, m.resolver_url);
                }
            }
        }
        Err(e) => println!("releases error: {e:?}"),
    }
    let rs = releases.expect("releases should parse");
    assert!(!rs.is_empty(), "should have at least one release");

    // 3. Resolve — try releases until one works (some hosts may be dead).
    println!("\n--- RESOLVE ---");
    let mut resolved_ok = false;
    for (idx, release) in rs.iter().enumerate().take(5) {
        println!(
            "trying release #{idx}: {} -> {} mirror(s)",
            release.filename,
            release.mirrors.len()
        );
        let resolved = client.resolve_release(release).await;
        match &resolved {
            Ok(src) => {
                println!("  RESOLVED: {}", src.url);
                println!("  label: {}", src.source_label);
                resolved_ok = true;
                break;
            }
            Err(e) => println!("  resolve error: {e}"),
        }
    }
    assert!(
        resolved_ok,
        "at least one release should resolve to a playable URL"
    );
}

#[tokio::test]
#[ignore = "hits live network; run with --ignored"]
async fn hdhub4u_series_releases() {
    let client = HdHub4uClient::new();
    // House of the Dragon S3 — a series with episode links
    let id = "/house-of-the-dragon-season-3-hindi-webrip-all-episodes/";
    println!("--- SERIES DETAILS: {id} ---");
    let details = client.details(id).await.expect("details should parse");
    println!("title: {}", details.title);
    println!("media_type: {:?}", details.media_type);
    println!("seasons: {}", details.seasons.len());
    if !details.seasons.is_empty() {
        let s = &details.seasons[0];
        println!("season {}: {} episodes", s.number, s.episodes.len());
        for ep in s.episodes.iter().take(3) {
            println!("  ep {} - {:?}", ep.number, ep.title);
        }
    }

    println!("\n--- SERIES RELEASES (s=1 e=1) ---");
    let releases = client
        .releases(id, 1, 1)
        .await
        .expect("releases should parse");
    println!("releases: {}", releases.len());
    for r in releases.iter().take(3) {
        println!(
            "  filename={:?} quality={:?} episode={:?} mirrors={}",
            r.filename,
            r.quality,
            r.episode,
            r.mirrors.len()
        );
    }
    assert!(!releases.is_empty(), "series should have releases for ep1");
}
