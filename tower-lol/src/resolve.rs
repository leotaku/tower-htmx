//! Middleware that resolves stored requests.

use std::collections::HashMap;
use std::future::Future;

use bytes::{BufMut, Bytes, BytesMut};
use http::{Request, Response};
use tower::{Layer, Service};

/// Newtype wrapper to hold responses in [`http::Extensions`].
pub struct ResolveContext {
    /// Map of response entries.
    pub entries: std::collections::HashMap<String, Option<Response<Bytes>>>,
}

impl ResolveContext {
    /// Create a new [`ResolveContext`] with no entries.
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

/// Layer to apply [`ResolveService`] middleware
#[derive(Debug, Clone)]
pub struct ResolveLayer {}

impl ResolveLayer {
    /// Create a new [`ResolveLayer`].
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

/// Middleware that resolves stored requests.
#[derive(Debug, Clone)]
pub struct ResolveService<S> {
    inner: S,
}

impl<S> ResolveService<S> {
    /// Create a new [`ResolveService`] middleware.
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

        async move {
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
                    .unwrap_or_else(|_| panic!("TODO"))
                {
                    buf.put(chunk);
                }

                *value = Some(inner_res.map(|_| buf.into()))
            }

            Ok(res)
        }
    }
}
