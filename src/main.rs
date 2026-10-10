use clap::Parser;
use salvo::__private::tracing;
use salvo::compression::Compression;
use salvo::fs::NamedFile;
use salvo::http::Method;
use salvo::http::header::{CACHE_CONTROL, HeaderValue, VARY};
use salvo::logging::Logger;
use salvo::prelude::*;
use salvo::routing::{decode_url_path, normalize_url_path};
use salvo::serve_static::StaticDir;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;

struct StaticFiles {
    primary: StaticDir,
    fallback_paths: Vec<PathBuf>,
}

#[handler]
impl StaticFiles {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        self.primary.handle(req, depot, res, ctrl).await;
        if res.status_code == Some(StatusCode::NOT_FOUND) {
            res.status_code = None;
            res.take_body();
            // 直接查找配置的回退文件，避免再次扫描原始请求路径。
            for path in &self.fallback_paths {
                if tokio::fs::symlink_metadata(path)
                    .await
                    .is_ok_and(|meta| meta.is_file())
                {
                    match NamedFile::builder(path).build().await {
                        Ok(file) => file.send(req.headers(), res).await,
                        Err(_) => res
                            .render(StatusError::internal_server_error().brief("read file failed")),
                    }
                    break;
                }
            }
            if res.status_code.is_none() {
                res.render(StatusError::not_found());
            }
            res.headers_mut()
                .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
        // 预压缩文件也需要声明协商维度，避免共享缓存混用压缩和原始内容。
        res.headers_mut()
            .insert(VARY, HeaderValue::from_static("accept-encoding"));
    }
}

struct ForceHttps {
    authority: String,
}

#[handler]
async fn method_not_allowed(res: &mut Response) {
    res.headers_mut().insert(
        salvo::http::header::ALLOW,
        HeaderValue::from_static("GET, HEAD"),
    );
    res.render(StatusError::method_not_allowed());
}

#[handler]
impl ForceHttps {
    async fn handle(&self, req: &mut Request, res: &mut Response, ctrl: &mut FlowCtrl) {
        if req.scheme() == &salvo::http::uri::Scheme::HTTP {
            let path = req
                .uri()
                .path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/");
            res.render(Redirect::permanent(format!(
                "https://{}{path}",
                self.authority
            )));
            ctrl.skip_rest();
        }
    }
}

struct NoCacheMiddleware {
    /// 启动时合并默认页、回退页和用户指定的文件名。
    filenames: HashSet<String>,
}

impl NoCacheMiddleware {
    fn should_no_cache(&self, path: &str) -> bool {
        let filename = path.rsplit('/').next().unwrap_or("");
        // 无扩展名路径（SPA 路由）也走 no-cache
        if !filename.contains('.') {
            return true;
        }
        // 用户自定义 pattern：精确匹配文件名
        self.filenames.contains(filename)
    }
}

#[handler]
impl NoCacheMiddleware {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        // 普通路径直接借用；只有编码路径才解码并规范化。
        let path = req.uri().path();
        let no_cache = if path.contains('%') || path.contains('\\') {
            self.should_no_cache(&normalize_url_path(&decode_url_path(path)))
        } else {
            self.should_no_cache(path)
        };
        ctrl.call_next(req, depot, res).await;
        if no_cache {
            res.headers_mut()
                .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
    }
}

/// Static server - 基于 Rust 的静态资源服务器
#[derive(Parser, Debug)]
#[command(name = "static-server")]
#[command(author = "Rust Static Server Team")]
#[command(version)]
#[command(about = "Rust 静态资源服务器，支持配置证书自动化等", long_about = None)]
struct Args {
    /// 静态服务监听端口
    #[arg(long, default_value = "0.0.0.0:8080")]
    http_addr: SocketAddr,

    /// 静态服务监听端口
    #[arg(long, default_value = "0.0.0.0:8443")]
    https_addr: SocketAddr,

    /// http3 开关
    #[arg(long, requires = "https_enable")]
    http3_enable: bool,

    /// HTTPS 开关
    #[arg(long, requires = "acme_domain")]
    https_enable: bool,

    /// 强制 HTTPS (HTTP 请求重定向到 HTTPS)
    #[arg(long, requires = "https_enable")]
    force_https: bool,

    /// HTTPS 重定向的外部端口（默认使用 HTTPS 监听端口）
    #[arg(long, requires = "force_https", value_parser = clap::value_parser!(u16).range(1..))]
    https_redirect_port: Option<u16>,

    /// 显式启用响应压缩（默认已启用）
    #[arg(long, conflicts_with = "no_compression")]
    compression: bool,

    /// 禁用响应压缩
    #[arg(long)]
    no_compression: bool,

    /// 自动签发证书
    #[arg(long)]
    acme_domain: Option<String>,

    /// 静态资源文件夹
    #[arg(long, default_values = &["static"])]
    static_dir: Vec<String>,

    /// 默认文件名 (当请求目录时)
    #[arg(long, default_value = "index.html")]
    try_file: String,

    /// 默认显示页面 (当找不到文件时)
    #[arg(long, default_value = "index.html")]
    default_page: String,

    /// 额外需要 Cache-Control: no-cache 的文件名，逗号分隔（精确匹配）
    /// 例如: --no-cache-patterns index.html,config.json
    #[arg(long, value_delimiter = ',', default_values = &["index.html"])]
    no_cache_patterns: Vec<String>,

    /// 日志级别 (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

fn initialize_logging(log_level: &str) {
    let level = match log_level.to_lowercase().as_str() {
        "trace" => Some(tracing::Level::TRACE),
        "debug" => Some(tracing::Level::DEBUG),
        "info" => Some(tracing::Level::INFO),
        "warn" => Some(tracing::Level::WARN),
        "error" => Some(tracing::Level::ERROR),
        _ => None,
    };

    tracing_subscriber::fmt()
        .with_max_level(level.unwrap_or(tracing::Level::INFO))
        .init();
    if level.is_none() {
        tracing::warn!(log_level, "无效的日志级别，使用默认值 'info'");
    }
}

fn build_router(args: &Args) -> Router {
    // 构建 no-cache 配置，try_file 和 default_page 强制加入
    let no_cache = NoCacheMiddleware {
        filenames: args
            .no_cache_patterns
            .iter()
            .cloned()
            .chain(
                [&args.try_file, &args.default_page]
                    .into_iter()
                    .map(|path| path.rsplit('/').next().unwrap_or("").to_owned()),
            )
            .collect(),
    };

    let primary = StaticDir::new(args.static_dir.clone())
        .include_dot_files(false)
        .defaults(args.try_file.clone())
        .auto_list(true);
    let fallback_paths = if args.default_page.is_empty() {
        Vec::new()
    } else {
        primary
            .roots
            .iter()
            .map(|root| root.join(normalize_url_path(&args.default_page)))
            .collect()
    };

    let mut static_router = Router::with_path("{*path}").goal(method_not_allowed).push(
        Router::new()
            .filter_fn(|req, _| matches!(*req.method(), Method::GET | Method::HEAD))
            .goal(StaticFiles {
                primary,
                fallback_paths,
            }),
    );

    let mut router = Router::new().hoop(Logger::new()).hoop(no_cache);

    // 静态路由覆盖所有请求方法；ACME 在根路由优先匹配，不经过重定向。
    if args.force_https {
        let port = args.https_redirect_port.unwrap_or(args.https_addr.port());
        let domain = args
            .acme_domain
            .as_deref()
            .expect("HTTPS 必须配置 ACME 域名");
        let authority = if port == 443 {
            domain.to_string()
        } else {
            format!("{domain}:{port}")
        };
        static_router = static_router.hoop(ForceHttps { authority });
    }

    if !args.no_compression {
        router = router.push(Router::with_hoop(Compression::new()).push(static_router));
    } else {
        router = router.push(static_router);
    }

    router
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    initialize_logging(&args.log_level);
    let mut router = build_router(&args);

    if args.https_enable {
        if let Some(ref acme_domain) = args.acme_domain {
            let https_listener = TcpListener::new(args.https_addr)
                .acme()
                .cache_path("temp/letsencrypt")
                .add_domain(acme_domain.clone())
                .http01_challenge(&mut router);

            if args.http3_enable {
                let acceptor = https_listener
                    .quinn(args.https_addr)
                    .join(TcpListener::new(args.http_addr))
                    .bind()
                    .await;
                Server::new(acceptor).serve(router).await;
            } else {
                let acceptor = https_listener
                    .join(TcpListener::new(args.http_addr))
                    .bind()
                    .await;
                Server::new(acceptor).serve(router).await;
            }
        } else {
            tracing::error!("启用 HTTPS 时必须提供 --acme-domain 参数");
            std::process::exit(1);
        }
    } else {
        // 仅 HTTP 模式
        let acceptor = TcpListener::new(args.http_addr).bind().await;
        Server::new(acceptor).serve(router).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use salvo::http::header::{CONTENT_ENCODING, LOCATION};
    use salvo::test::{ResponseExt, TestClient};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

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
        assert!(
            Args::try_parse_from(["static-server", "--compression", "--no-compression"]).is_err()
        );
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
        // 预压缩版本用不同内容确认响应来自变体文件，而不是动态压缩。
        let compressed = [
            31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 43, 40, 74, 77, 206, 207, 45, 40, 74, 45, 46, 78,
            77, 81, 40, 75, 44, 202, 76, 204, 43, 1, 0, 173, 133, 111, 56, 21, 0, 0, 0,
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
        // take_bytes 不对 Content-Encoding 解压，验证原始变体内容。
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
            let response =
                TestClient::get("http://untrusted.example/users/jane.doe?tab=1&sort=asc")
                    .send(&service)
                    .await;
            assert_eq!(response.status_code, Some(StatusCode::PERMANENT_REDIRECT));
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
            let mut response =
                TestClient::get("http://example.com/.well-known/acme-challenge/unknown")
                    .send(&service)
                    .await;
            assert!(response.headers().get(LOCATION).is_none());
            // Salvo 的 ACME 处理器在 token 未注册时返回 token 本身。
            assert_eq!(response.take_string().await.unwrap(), "unknown");
        }
    }
}
