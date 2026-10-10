use salvo::prelude::*;

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
