use std::{collections::HashMap, future::Future, pin::Pin};

use bytes::{BufMut, Bytes, BytesMut};
use http::{Request, Response, Uri};
use tower::{Layer, Service};

pub struct ResolveContext {
    pub entries: std::collections::HashMap<String, Option<Bytes>>,
}

impl ResolveContext {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolveLayer {}

impl ResolveLayer {
    pub fn new() -> Self {
        Self {}
    }
}

impl<S> Layer<S> for ResolveLayer {
    type Service = ResolveService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResolveService::new(inner)
    }
}

#[derive(Debug, Clone)]
pub struct ResolveService<S> {
    inner: S,
}

impl<S> ResolveService<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for ResolveService<S>
where
    S: Service<Request<ReqBody>> + Clone + Send + 'static,
    S::Future: Future<Output = Result<Response<ResBody>, S::Error>> + Send,
    ReqBody: Default + Send + 'static,
    ResBody: http_body::Body + Send + Unpin,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut cloned = self.clone();
        let uri = dbg!(req.uri()).clone();
        let res = self.inner.call(req);

        Box::pin(async move {
            std::future::poll_fn(|cx| cloned.poll_ready(cx)).await?;

            let mut res = res.await?;
            let ctx: &mut ResolveContext = match res.extensions_mut().get_mut() {
                Some(some) => some,
                None => return Ok(res),
            };

            for (key, value) in ctx.entries.iter_mut() {
                let mut inner_res = cloned
                    .inner
                    .call(
                        Request::builder()
                            .method("GET")
                            .uri(format!("/{key}"))
                            .body(Default::default())
                            .unwrap(),
                    )
                    .await?;

                let mut buf = BytesMut::new();
                while let Some(chunk) = inner_res
                    .body_mut()
                    .data()
                    .await
                    .transpose()
                    .unwrap_or_else(|_| panic!())
                {
                    buf.put(chunk);
                }

                *value = Some(buf.into())
            }

            Ok(res)
        })
    }
}
