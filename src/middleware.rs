use salvo::http::header::{CACHE_CONTROL, HeaderValue};
use salvo::prelude::*;
use salvo::routing::{decode_url_path, normalize_url_path};
use std::collections::HashSet;

pub(crate) struct ForceHttps {
    authority: String,
}

impl ForceHttps {
    pub(crate) fn new(authority: String) -> Self {
        Self { authority }
    }
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

pub(crate) struct NoCacheMiddleware {
    /// Default, fallback, and user-specified filenames collected at startup.
    filenames: HashSet<String>,
}

impl NoCacheMiddleware {
    pub(crate) fn new(filenames: impl IntoIterator<Item = String>) -> Self {
        Self {
            filenames: filenames.into_iter().collect(),
        }
    }

    fn should_no_cache(&self, path: &str) -> bool {
        let filename = path.rsplit('/').next().unwrap_or("");
        // Extensionless paths (SPA routes) also require revalidation.
        if !filename.contains('.') {
            return true;
        }
        // Match configured filenames exactly.
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
        // Borrow plain paths; decode and normalize only encoded paths.
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
