//! TODO

use bytes::{BufMut, Bytes, BytesMut};
use http::{Request, Response};
use std::collections::HashMap;
use std::future::Future;
use tower::{Layer, Service};

/// TODO
pub struct ResolveContext {
    /// TODO
    pub entries: std::collections::HashMap<String, Option<Bytes>>,
}

impl ResolveContext {
    /// TODO
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl Default for ResolveContext {
    fn default() -> Self {
        Self::new()
    }
}

/// TODO
#[derive(Debug, Clone)]
pub struct ResolveLayer {}

impl ResolveLayer {
    /// TODO
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ResolveLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for ResolveLayer {
    type Service = ResolveService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ResolveService::new(inner)
    }
}

#[derive(Debug, Clone)]
/// TODO
pub struct ResolveService<S> {
    inner: S,
}

impl<S> ResolveService<S> {
    /// TODO
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for ResolveService<S>
where
    S: Service<Request<ReqBody>> + Clone,
    S::Future: Future<Output = Result<Response<ResBody>, S::Error>>,
    ReqBody: Default,
    ResBody: http_body::Body + Unpin,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = impl Future<Output = Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut cloned = std::mem::replace(self, self.clone());

        Box::pin(async move {
            let mut res = cloned.inner.call(req).await?;
            let ctx: &mut ResolveContext = match res.extensions_mut().get_mut() {
                Some(some) => some,
                None => return Ok(res),
            };

            for (key, value) in ctx.entries.iter_mut() {
                std::future::poll_fn(|cx| cloned.poll_ready(cx)).await?;
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
