use salvo::http::header::{CACHE_CONTROL, HeaderValue};
use salvo::prelude::*;
use salvo::routing::{decode_url_path, normalize_url_path};
use std::collections::HashSet;
use std::str::FromStr;

#[derive(Clone, Debug)]
pub(crate) struct CachePolicy {
    value: HeaderValue,
    no_store: bool,
}

impl FromStr for CachePolicy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("Cache-Control must not be empty".into());
        }
        let header = HeaderValue::from_str(value)
            .map_err(|_| "Cache-Control contains invalid header characters")?;
        let no_store = value
            .split(',')
            .any(|directive| directive.trim().eq_ignore_ascii_case("no-store"));
        Ok(Self {
            value: header,
            no_store,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CacheRule {
    pattern: String,
    policy: CachePolicy,
}

impl FromStr for CacheRule {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (pattern, policy) = value
            .split_once('=')
            .ok_or("Cache rules must use /path/pattern=Cache-Control")?;
        if !pattern.starts_with('/') || pattern.contains(['?', '#', '\\']) {
            return Err("Cache rule patterns must start with / and contain no query, fragment, or backslash".into());
        }
        Ok(Self {
            pattern: pattern.into(),
            policy: policy.parse()?,
        })
    }
}

// A star matches any number of bytes, including path separators.
fn matches_path(pattern: &str, path: &str) -> bool {
    let (pattern, path) = (pattern.as_bytes(), path.as_bytes());
    let (mut p, mut s) = (0, 0);
    let mut star = None;
    let mut retry = 0;
    while s < path.len() {
        if p < pattern.len() && pattern[p] != b'*' && pattern[p] == path[s] {
            p += 1;
            s += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = s;
        } else if let Some(index) = star {
            p = index + 1;
            retry += 1;
            s = retry;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

pub(crate) struct CacheControl {
    filenames: HashSet<String>,
    default: Option<CachePolicy>,
    rules: Vec<CacheRule>,
}

impl CacheControl {
    pub(crate) fn new(
        filenames: impl IntoIterator<Item = String>,
        default: Option<CachePolicy>,
        rules: Vec<CacheRule>,
    ) -> Self {
        Self {
            filenames: filenames.into_iter().collect(),
            default,
            rules,
        }
    }
}

#[handler]
impl CacheControl {
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        let path = format!(
            "/{}",
            normalize_url_path(&decode_url_path(req.uri().path()))
        );
        let filename = path.rsplit('/').next().unwrap_or("");
        let protected = !filename.contains('.') || self.filenames.contains(filename);
        let policy = self
            .rules
            .iter()
            .rev()
            .find(|rule| matches_path(&rule.pattern, &path))
            .map(|rule| &rule.policy)
            .or(self.default.as_ref());
        ctrl.call_next(req, depot, res).await;
        if !matches!(
            res.status_code,
            Some(StatusCode::OK | StatusCode::PARTIAL_CONTENT | StatusCode::NOT_MODIFIED)
        ) {
            return;
        }
        // The static handler marks fallback responses with no-cache.
        let protected = protected
            || res
                .headers()
                .get(CACHE_CONTROL)
                .is_some_and(|value| value == "no-cache");
        let value = if protected && !policy.is_some_and(|policy| policy.no_store) {
            Some(HeaderValue::from_static("no-cache"))
        } else {
            policy.map(|policy| policy.value.clone())
        };
        if let Some(value) = value {
            res.headers_mut().insert(CACHE_CONTROL, value);
        }
    }
}
