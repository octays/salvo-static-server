use salvo::fs::NamedFile;
use salvo::http::header::{CACHE_CONTROL, HeaderValue, VARY};
use salvo::prelude::*;
use salvo::routing::normalize_url_path;
use salvo::serve_static::StaticDir;
use std::path::PathBuf;

pub(crate) struct StaticFiles {
    primary: StaticDir,
    fallback_paths: Vec<PathBuf>,
}

impl StaticFiles {
    pub(crate) fn new(roots: &[String], try_file: &str, default_page: &str) -> Self {
        let primary = StaticDir::new(roots.to_vec())
            .include_dot_files(false)
            .defaults(try_file)
            .auto_list(true);
        let fallback_paths = if default_page.is_empty() {
            Vec::new()
        } else {
            let fallback = normalize_url_path(default_page);
            primary
                .roots
                .iter()
                .map(|root| root.join(&fallback))
                .collect()
        };
        Self {
            primary,
            fallback_paths,
        }
    }
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
            // Look up fallback files directly without scanning the request path again.
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
            if matches!(
                res.status_code,
                Some(StatusCode::OK | StatusCode::PARTIAL_CONTENT | StatusCode::NOT_MODIFIED)
            ) {
                res.headers_mut()
                    .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            }
        }
        // Vary on encoding so shared caches distinguish compressed and original content.
        res.headers_mut()
            .insert(VARY, HeaderValue::from_static("accept-encoding"));
    }
}

#[handler]
pub(crate) async fn method_not_allowed(res: &mut Response) {
    res.headers_mut().insert(
        salvo::http::header::ALLOW,
        HeaderValue::from_static("GET, HEAD"),
    );
    res.render(StatusError::method_not_allowed());
}
