use crate::args::Args;
use crate::cache::CacheControl;
use crate::middleware::ForceHttps;
use crate::static_files::{StaticFiles, method_not_allowed};
use salvo::compression::Compression;
use salvo::http::Method;
use salvo::logging::Logger;
use salvo::prelude::*;

pub(crate) fn build_router(args: &Args) -> Router {
    // Always include the default and fallback pages in the no-cache configuration.
    let cache = CacheControl::new(
        args.no_cache_patterns.iter().cloned().chain(
            [&args.try_file, &args.default_page]
                .into_iter()
                .map(|path| path.rsplit('/').next().unwrap_or("").to_owned()),
        ),
        args.cache_control.clone(),
        args.cache_rules.clone(),
    );

    let mut static_router = Router::with_path("{*path}").goal(method_not_allowed).push(
        Router::new()
            .filter_fn(|req, _| matches!(*req.method(), Method::GET | Method::HEAD))
            .hoop(cache)
            .goal(StaticFiles::new(
                &args.static_dir,
                &args.try_file,
                &args.default_page,
            )),
    );

    let mut router = Router::new().hoop(Logger::new());

    // The static route covers all methods; ACME matches first and bypasses redirects.
    if args.force_https {
        let port = args.https_redirect_port.unwrap_or(args.https_addr.port());
        let domain = args
            .acme_domain
            .as_deref()
            .expect("HTTPS requires an ACME domain");
        let authority = if port == 443 {
            domain.to_string()
        } else {
            format!("{domain}:{port}")
        };
        static_router = static_router.hoop(ForceHttps::new(authority));
    }

    if !args.no_compression {
        router = router.push(Router::with_hoop(Compression::new()).push(static_router));
    } else {
        router = router.push(static_router);
    }

    router
}
