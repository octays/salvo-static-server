use crate::args::Args;
use crate::router::build_router;
use salvo::__private::tracing;
use salvo::prelude::*;

pub(crate) async fn run(args: Args) {
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
            tracing::error!("HTTPS requires the --acme-domain option");
            std::process::exit(1);
        }
    } else {
        // HTTP-only mode
        let acceptor = TcpListener::new(args.http_addr).bind().await;
        Server::new(acceptor).serve(router).await;
    }
}
