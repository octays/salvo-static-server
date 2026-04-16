use clap::Parser;
use salvo::__private::tracing;
use salvo::compression::Compression;
use salvo::http::header::{CACHE_CONTROL, HeaderValue};
use salvo::logging::Logger;
use salvo::prelude::*;
use salvo::serve_static::StaticDir;

struct NoCacheMiddleware {
    /// try_file 和 default_page 的文件名（必须 no-cache）
    required: Vec<String>,
    /// 用户通过 --no-cache-patterns 额外指定的后缀或文件名
    patterns: Vec<String>,
}

impl NoCacheMiddleware {
    fn should_no_cache(&self, path: &str) -> bool {
        let filename = path.split('/').last().unwrap_or("");
        // try_file / default_page 必须 no-cache
        if self.required.iter().any(|r| filename == r.as_str()) {
            return true;
        }
        // 无扩展名路径（SPA 路由）也走 no-cache
        if !filename.contains('.') {
            return true;
        }
        // 用户自定义 pattern：精确匹配文件名
        self.patterns.iter().any(|p| filename == p.as_str())
    }
}

#[handler]
impl NoCacheMiddleware {
    async fn handle(&self, req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
        let path = req.uri().path().to_string();
        ctrl.call_next(req, depot, res).await;
        if self.should_no_cache(&path) {
            res.headers_mut()
                .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
    }
}

/// Static server - 基于 Rust 的静态资源服务器
#[derive(Parser, Debug)]
#[command(name = "r-static")]
#[command(author = "Rust Static Server Team")]
#[command(version = "0.2.0")]
#[command(about = "Rust 静态资源服务器，支持配置证书自动化等", long_about = None)]
struct Args {
    /// 静态服务监听端口
    #[arg(long, default_value = "0.0.0.0:8080")]
    http_addr: String,

    /// 静态服务监听端口
    #[arg(long, default_value = "0.0.0.0:8443")]
    https_addr: String,

    /// http3 开关
    #[arg(long)]
    http3_enable: bool,

    /// HTTPS 开关
    #[arg(long)]
    https_enable: bool,

    /// 强制 HTTPS (HTTP 请求重定向到 HTTPS)
    #[arg(long)]
    force_https: bool,

    /// 响应压缩 (默认启用)
    #[arg(long)]
    compression: bool,

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
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => {
            eprintln!("无效的日志级别 '{}', 使用默认值 'info'", log_level);
            tracing::Level::INFO
        }
    };

    tracing_subscriber::fmt().with_max_level(level).init();
}

#[tokio::main]
async fn main() {
    // 解析命令行参数
    let args = Args::parse();
    initialize_logging(&args.log_level);

    // 构建 no-cache 配置，try_file 和 default_page 强制加入
    let mut required = vec![args.try_file.clone(), args.default_page.clone()];
    required.dedup();
    let no_cache = NoCacheMiddleware {
        required,
        patterns: args.no_cache_patterns.clone(),
    };

    let static_router = Router::with_path("{*path}").get(
        StaticDir::new(args.static_dir)
            .include_dot_files(false)
            .defaults(args.try_file)
            .fallback(args.default_page)
            .auto_list(true),
    );

    let mut router = Router::new()
        .hoop(Logger::new())
        .hoop(no_cache);

    if args.compression {
        router = router.push(Router::with_hoop(Compression::new()).push(static_router));
    } else {
        router = router.push(static_router);
    }

    if args.https_enable {
        if let Some(ref acme_domain) = args.acme_domain {
            let https_listener = TcpListener::new(args.https_addr.clone())
                .acme()
                .cache_path("temp/letsencrypt")
                .add_domain(acme_domain.clone())
                .http01_challenge(&mut router);

            if args.http3_enable {
                let acceptor = https_listener
                    .quinn(args.https_addr.clone())
                    .join(TcpListener::new(args.http_addr.clone()))
                    .bind()
                    .await;
                Server::new(acceptor).serve(router).await;
            } else {
                let acceptor = https_listener
                    .join(TcpListener::new(args.http_addr.clone()))
                    .bind()
                    .await;
                Server::new(acceptor).serve(router).await;
            }
        } else {
            eprintln!("错误：启用 HTTPS 时必须提供 --acme-domain 参数");
            std::process::exit(1);
        }
    } else {
        // 仅 HTTP 模式
        let acceptor = TcpListener::new(args.http_addr.clone()).bind().await;
        Server::new(acceptor).serve(router).await;
    }
}
