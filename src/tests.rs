use crate::args::Args;
use crate::router::build_router;
use clap::Parser;
use salvo::http::header::{CACHE_CONTROL, CONTENT_ENCODING, LOCATION, VARY};
use salvo::prelude::*;
use salvo::test::{ResponseExt, TestClient};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

#[test]
fn validates_cache_configuration() {
    for value in ["", "public\r\nx-test: injected"] {
        assert!(Args::try_parse_from(["static-server", "--cache-control", value]).is_err());
    }
    for value in [
        "/app.js",
        "app.js=no-cache",
        "/app.js?x=1=no-cache",
        "/app.js=",
    ] {
        assert!(Args::try_parse_from(["static-server", "--cache-rule", value]).is_err());
    }
}

#[tokio::test]
async fn cache_rules_respect_precedence_and_protected_responses() {
    let fixture = Fixture::new();
    let service = Service::new(build_router(&fixture.args(&[
        "--no-compression",
        "--cache-control",
        "public, max-age=3600",
        "--cache-rule",
        "/*=public, max-age=31536000, immutable",
        "--cache-rule",
        "/app*.js=public, max-age=600",
        "--cache-rule",
        "/config.json=no-store",
        "--cache-rule",
        "/private/*=no-store",
    ])));
    for (path, expected) in [
        ("/app.js", "public, max-age=600"),
        ("/app.js?version=1", "public, max-age=600"),
        ("/a%70p.js", "public, max-age=600"),
        ("/config.json", "no-store"),
        ("/index.html", "no-cache"),
        ("/", "no-cache"),
        ("/missing.js", "no-cache"),
        ("/private/missing.js", "no-store"),
    ] {
        let response = TestClient::get(format!("http://localhost{path}"))
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::OK), "{path}");
        assert_eq!(response.headers()[CACHE_CONTROL], expected, "{path}");
    }
    let get = TestClient::get("http://localhost/app.js")
        .send(&service)
        .await;
    let head = TestClient::head("http://localhost/app.js")
        .send(&service)
        .await;
    let conditional = TestClient::get("http://localhost/app.js")
        .add_header("if-none-match", get.headers()["etag"].clone(), true)
        .send(&service)
        .await;
    let range = TestClient::get("http://localhost/app.js")
        .add_header("range", "bytes=0-5", true)
        .send(&service)
        .await;
    assert_eq!(conditional.status_code, Some(StatusCode::NOT_MODIFIED));
    assert_eq!(range.status_code, Some(StatusCode::PARTIAL_CONTENT));
    for response in [head, conditional, range] {
        assert_eq!(response.headers()[CACHE_CONTROL], "public, max-age=600");
    }
    let service = Service::new(build_router(&fixture.args(&[
        "--cache-control",
        "public, max-age=3600",
        "--no-cache-patterns",
        "config.json",
        "--default-page",
        "absent.html",
    ])));
    for (path, expected) in [
        ("/app.js", "public, max-age=3600"),
        ("/config.json", "no-cache"),
    ] {
        let response = TestClient::get(format!("http://localhost{path}"))
            .send(&service)
            .await;
        assert_eq!(response.headers()[CACHE_CONTROL], expected);
    }
    let missing = TestClient::get("http://localhost/missing.js")
        .send(&service)
        .await;
    assert_eq!(missing.status_code, Some(StatusCode::NOT_FOUND));
    assert!(missing.headers().get(CACHE_CONTROL).is_none());
    let post = TestClient::post("http://localhost/app.js")
        .send(&service)
        .await;
    assert!(post.headers().get(CACHE_CONTROL).is_none());
    let service = Service::new(build_router(&fixture.args(&[
        "--cache-control",
        "no-store",
        "--no-cache-patterns",
        "config.json",
    ])));
    for path in ["/", "/index.html", "/config.json", "/missing.js"] {
        let response = TestClient::get(format!("http://localhost{path}"))
            .send(&service)
            .await;
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store", "{path}");
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "static-server-{}-{unique}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            path.join("index.html"),
            "<html>SPA entry</html>".repeat(100),
        )
        .unwrap();
        std::fs::write(path.join("app.js"), "console.log('asset');".repeat(100)).unwrap();
        std::fs::write(path.join("config.json"), "{}").unwrap();
        Self(path)
    }

    fn args(&self, extra: &[&str]) -> Args {
        let mut values = vec!["static-server", "--static-dir", self.0.to_str().unwrap()];
        values.extend_from_slice(extra);
        Args::try_parse_from(values).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn validates_cli_flags() {
    let defaults = Args::try_parse_from(["static-server"]).unwrap();
    assert!(!defaults.no_compression);
    assert!(
        Args::try_parse_from(["static-server", "--no-compression"])
            .unwrap()
            .no_compression
    );
    assert!(
        Args::try_parse_from(["static-server", "--compression"])
            .unwrap()
            .compression
    );
    assert!(Args::try_parse_from(["static-server", "--compression", "--no-compression"]).is_err());
    assert!(Args::try_parse_from(["static-server", "--force-https"]).is_err());
    assert!(Args::try_parse_from(["static-server", "--https-enable"]).is_err());
    assert!(Args::try_parse_from(["static-server", "--http3-enable"]).is_err());
    assert!(Args::try_parse_from(["static-server", "--http-addr", "invalid"]).is_err());
    assert!(Args::try_parse_from(["static-server", "--https-addr", "[::1]:8443"]).is_ok());
    assert!(
        Args::try_parse_from([
            "static-server",
            "--https-enable",
            "--acme-domain",
            "example.com",
            "--force-https",
            "--https-redirect-port",
            "0"
        ])
        .is_err()
    );
    let version = Args::try_parse_from(["static-server", "--version"]).unwrap_err();
    assert!(version.to_string().contains(env!("CARGO_PKG_VERSION")));
}

#[tokio::test]
async fn cache_control_tracks_fallback_pages() {
    let fixture = Fixture::new();
    let service = Service::new(build_router(
        &fixture.args(&["--no-cache-patterns", "config.json"]),
    ));
    for path in [
        "/",
        "/index.html",
        "/users/jane.doe",
        "/missing.js",
        "/dashboard",
    ] {
        let mut response = TestClient::get(format!("http://localhost{path}"))
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::OK), "{path}");
        assert_eq!(response.headers()[CACHE_CONTROL], "no-cache", "{path}");
        assert!(response.take_string().await.unwrap().contains("SPA entry"));
    }
    let response = TestClient::get("http://localhost/config.json")
        .send(&service)
        .await;
    assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
    let mut encoded = TestClient::get("http://localhost/confi%67.json")
        .send(&service)
        .await;
    assert_eq!(encoded.headers()[CACHE_CONTROL], "no-cache");
    assert_eq!(encoded.take_string().await.unwrap(), "{}");
    let response = TestClient::get("http://localhost/app.js")
        .send(&service)
        .await;
    assert!(response.headers().get(CACHE_CONTROL).is_none());

    std::fs::write(fixture.0.join("fallback.txt"), "custom fallback").unwrap();
    let service = Service::new(build_router(
        &fixture.args(&["--default-page", "fallback.txt"]),
    ));
    let mut response = TestClient::get("http://localhost/missing.css")
        .send(&service)
        .await;
    assert_eq!(response.headers()[CACHE_CONTROL], "no-cache");
    assert_eq!(response.take_string().await.unwrap(), "custom fallback");
}

#[tokio::test]
async fn head_and_fallback_preserve_file_semantics() {
    let fixture = Fixture::new();
    let service = Service::new(build_router(&fixture.args(&["--no-compression"])));
    for path in ["/index.html", "/missing.js"] {
        let get = TestClient::get(format!("http://localhost{path}"))
            .send(&service)
            .await;
        let mut head = TestClient::head(format!("http://localhost{path}"))
            .send(&service)
            .await;
        assert_eq!(head.status_code, Some(StatusCode::OK));
        for header in ["content-type", "content-length", "etag", "cache-control"] {
            assert_eq!(
                head.headers().get(header),
                get.headers().get(header),
                "{header}"
            );
        }
        assert!(head.take_string().await.unwrap().is_empty());
        let mut conditional = TestClient::get(format!("http://localhost{path}"))
            .add_header("if-none-match", get.headers()["etag"].clone(), true)
            .send(&service)
            .await;
        assert_eq!(conditional.status_code, Some(StatusCode::NOT_MODIFIED));
        assert!(conditional.take_string().await.unwrap().is_empty());
        let mut range = TestClient::get(format!("http://localhost{path}"))
            .add_header("range", "bytes=0-5", true)
            .send(&service)
            .await;
        assert_eq!(range.status_code, Some(StatusCode::PARTIAL_CONTENT));
        assert_eq!(range.take_string().await.unwrap(), "<html>");
    }
    let post = TestClient::post("http://localhost/index.html")
        .send(&service)
        .await;
    assert_eq!(post.status_code, Some(StatusCode::METHOD_NOT_ALLOWED));
    assert_eq!(post.headers()["allow"], "GET, HEAD");
    let service = Service::new(build_router(
        &fixture.args(&["--default-page", "absent.html"]),
    ));
    let response = TestClient::get("http://localhost/missing.js")
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::NOT_FOUND));
}

#[tokio::test]
async fn fallback_checks_roots_in_order() {
    let first = Fixture::new();
    let second = Fixture::new();
    std::fs::write(second.0.join("fallback.txt"), "second root").unwrap();
    let args = first.args(&[
        "--static-dir",
        second.0.to_str().unwrap(),
        "--default-page",
        "fallback.txt",
    ]);
    let service = Service::new(build_router(&args));
    let mut response = TestClient::get("http://localhost/missing.js")
        .send(&service)
        .await;
    assert_eq!(response.take_string().await.unwrap(), "second root");
    std::fs::write(first.0.join("fallback.txt"), "first root").unwrap();
    let mut response = TestClient::get("http://localhost/missing.js")
        .send(&service)
        .await;
    assert_eq!(response.take_string().await.unwrap(), "first root");
}

#[tokio::test]
async fn serves_precompressed_assets_without_dynamic_compression() {
    let fixture = Fixture::new();
    // Use distinct content to verify that the response comes from the precompressed file.
    let compressed = [
        31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 43, 40, 74, 77, 206, 207, 45, 40, 74, 45, 46, 78, 77,
        81, 40, 75, 44, 202, 76, 204, 43, 1, 0, 173, 133, 111, 56, 21, 0, 0, 0,
    ];
    std::fs::write(fixture.0.join("app.js.gz"), compressed).unwrap();
    let service = Service::new(build_router(&fixture.args(&["--no-compression"])));
    let mut response = TestClient::get("http://localhost/app.js")
        .add_header("accept-encoding", "gzip", true)
        .send(&service)
        .await;
    assert_eq!(response.headers()[CONTENT_ENCODING], "gzip");
    assert_eq!(response.headers()["content-length"], "41");
    assert_eq!(response.headers()[VARY], "accept-encoding");
    // take_bytes does not decode Content-Encoding; verify the raw variant bytes.
    assert_eq!(
        response.take_bytes(None).await.unwrap().as_ref(),
        compressed
    );
    let response = TestClient::get("http://localhost/app.js")
        .send(&service)
        .await;
    assert!(response.headers().get(CONTENT_ENCODING).is_none());
    assert_eq!(response.headers()[VARY], "accept-encoding");
}

#[tokio::test]
async fn compression_is_enabled_by_default_and_can_be_disabled() {
    let fixture = Fixture::new();
    for (flags, enabled) in [(vec![], true), (vec!["--no-compression"], false)] {
        let service = Service::new(build_router(&fixture.args(&flags)));
        let response = TestClient::get("http://localhost/app.js")
            .add_header("accept-encoding", "gzip", true)
            .send(&service)
            .await;
        assert_eq!(response.headers().get(CONTENT_ENCODING).is_some(), enabled);
        if enabled {
            assert_eq!(response.headers()[CONTENT_ENCODING], "gzip");
        }
    }
}

#[tokio::test]
async fn redirects_http_but_preserves_https_and_acme() {
    let fixture = Fixture::new();
    for (port_flags, authority) in [
        (vec![], "example.com:8443"),
        (vec!["--https-redirect-port", "443"], "example.com"),
    ] {
        let mut flags = vec![
            "--cache-control",
            "public, max-age=3600",
            "--https-enable",
            "--acme-domain",
            "example.com",
            "--force-https",
        ];
        flags.extend(port_flags);
        let args = fixture.args(&flags);
        let mut router = build_router(&args);
        let _listener = TcpListener::new(args.https_addr)
            .acme()
            .add_domain("example.com")
            .http01_challenge(&mut router);
        let service = Service::new(router);
        let response = TestClient::get("http://untrusted.example/users/jane.doe?tab=1&sort=asc")
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::PERMANENT_REDIRECT));
        assert!(response.headers().get(CACHE_CONTROL).is_none());
        assert_eq!(
            response.headers()[LOCATION],
            format!("https://{authority}/users/jane.doe?tab=1&sort=asc")
        );
        let response = TestClient::post("http://example.com/index.html")
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::PERMANENT_REDIRECT));
        let response = TestClient::get("https://example.com/index.html")
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        assert!(response.headers().get(LOCATION).is_none());
        let mut response = TestClient::get("http://example.com/.well-known/acme-challenge/unknown")
            .send(&service)
            .await;
        assert!(response.headers().get(LOCATION).is_none());
        // Salvo returns the token itself when no ACME challenge key is registered.
        assert!(response.headers().get(CACHE_CONTROL).is_none());
        assert_eq!(response.take_string().await.unwrap(), "unknown");
    }
}
