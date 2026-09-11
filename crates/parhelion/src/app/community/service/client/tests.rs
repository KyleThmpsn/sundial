use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
};

#[test]
fn community_transport_uploads_json_and_checks_pinned_downloads() {
    let downloaded = super::super::tests::downloaded();
    let raw = downloaded.recipe.to_json_pretty().unwrap();
    let catalog = serde_json::to_string(&Catalog {
        schema: 1,
        recipes: vec![downloaded.entry.clone()],
    })
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/api/parhelion", listener.local_addr().unwrap());
    let digest = downloaded.entry.sha256.clone();
    let server = std::thread::spawn(move || {
        for (path, body) in [
            ("/api/parhelion/catalog".to_owned(), catalog),
            (
                format!(
                    "/api/parhelion/recipes/community-test/recipe.parhelion.json?sha256={digest}"
                ),
                raw,
            ),
            (
                "/api/parhelion/recipes/community-test/downloads".to_owned(),
                String::new(),
            ),
            (
                "/api/parhelion/recipes".to_owned(),
                r#"{"id":"submission-reference"}"#.into(),
            ),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request).unwrap();
            assert!(headers.lines().next().unwrap().contains(&path));
            assert!(!headers.to_lowercase().contains("authorization:"));
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            if length > 0 {
                let mut payload = vec![0; length];
                stream.read_exact(&mut payload).unwrap();
                let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                if path.ends_with("/downloads") {
                    assert_eq!(value["sha256"], digest);
                } else {
                    assert!(value["recipe"].is_string());
                    assert_eq!(value["listing"]["id"], "community-test");
                }
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let client = Client {
        agent: ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .into(),
        endpoint,
    };
    let catalog = client.catalog().unwrap();
    assert_eq!(
        client.download(&catalog.recipes[0]).unwrap().recipe,
        downloaded.recipe
    );
    client.record_download(&downloaded.entry).unwrap();
    assert_eq!(
        client
            .submit(&Submission {
                listing: downloaded.entry.listing,
                recipe: downloaded.recipe
            })
            .unwrap(),
        "submission-reference"
    );
    server.join().unwrap();
}
