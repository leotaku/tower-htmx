//! Middlewares for building sites using [`htmx`] and [`tower`].
//!
//! [`htmx`]: https://htmx.org/reference/

#![feature(impl_trait_in_assoc_type)]
#![forbid(unused_unsafe)]
#![warn(clippy::all, missing_docs, nonstandard_style, future_incompatible)]
#![allow(clippy::type_complexity)]

mod error;
mod future;
mod rewriter;

use std::future::{poll_fn, Future};
use std::mem;

use bytes::Bytes;
use error::{Error, HandleErrorService};
use http::{Request, Response};
use http_body_util::BodyExt;
use rewriter::Rewriter;
use tower::{Layer, Service};

/// Layer to apply [`HtmxRewriteService`] middleware.
#[derive(Debug, Clone)]
pub struct HtmxRewriteLayer;

impl HtmxRewriteLayer {
    /// Create a new [`HtmxRewriteLayer`].
    pub fn new() -> Self {
        HtmxRewriteLayer
    }

    /// Convert this layer into a [`HtmxRewriteLayerInfallible`].
    pub fn infallible(self) -> HtmxRewriteLayerInfallible {
        HtmxRewriteLayerInfallible(self)
    }
}

impl<S> Layer<S> for HtmxRewriteLayer {
    type Service = HtmxRewriteService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HtmxRewriteService::new(inner)
    }
}

/// Layer to apply [`HtmxRewriteService`] middleware in-band error handling.
#[derive(Debug, Clone)]
pub struct HtmxRewriteLayerInfallible(HtmxRewriteLayer);

impl<S> Layer<S> for HtmxRewriteLayerInfallible {
    type Service = HandleErrorService<HtmxRewriteService<S>>;

    fn layer(&self, inner: S) -> Self::Service {
        HandleErrorService(self.0.layer(inner))
    }
}

/// Middleware that .
#[derive(Debug, Clone)]
pub struct HtmxRewriteService<S>(S);

impl<S> HtmxRewriteService<S> {
    /// Create a new [`HtmxRewriteService`] middleware.
    pub fn new(inner: S) -> Self {
        HtmxRewriteService(inner)
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for HtmxRewriteService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Default,
    ResBody: http_body::Body<Data = Bytes>,
{
    type Response = Response<http_body_util::Either<ResBody, http_body_util::Full<Bytes>>>;
    type Error = error::Error<S::Error, ResBody::Error, lol_html::errors::RewritingError>;
    type Future = impl Future<Output = Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0
            .poll_ready(cx)
            .map_err(|err| Error::Future(err.into()))
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut rw = rewriter::BasicRewriter::new();

        if !rw.match_request(&req) {
            let fut = self.0.call(req);
            return future::Either::left(async move {
                let rsp = fut.await.map_err(Error::Future)?;
                Ok(rsp.map(http_body_util::Either::Left))
            });
        };

        let fut = self.0.call(req);
        let mut service = mem::replace(self, self.clone());

        future::Either::right(Box::pin(async move {
            let res = fut.await.map_err(Error::Future)?;
            if !rw.match_response(&res) {
                return Ok(res.map(http_body_util::Either::Left));
            };

            let (mut parts, body) = res.into_parts();
            let original = body.collect().await.map_err(Error::Body)?.to_bytes();
            let reqs = rw.extract_info(&original).map_err(Error::Rewrite)?;

            let mut resps = Vec::new();
            for req in reqs {
                poll_fn(|cx| service.poll_ready(cx)).await?;
                let req = req
                    .body(ReqBody::default())
                    .expect("request to always be constructible");

                let (parts, body) = service.call(req).await?.into_parts();
                let body = match body {
                    http_body_util::Either::Left(left) => {
                        left.collect().await.map_err(Error::Body)?
                    }
                    http_body_util::Either::Right(right) => right
                        .collect()
                        .await
                        .expect("Full body to always be collectable"),
                };

                resps.push(Response::from_parts(parts, body.to_bytes()));
            }

            let rewritten = rw.rewrite(&original, resps).map_err(Error::Rewrite)?;

            parts
                .headers
                .insert(http::header::CONTENT_LENGTH, rewritten.len().into());

            Ok(Response::from_parts(
                parts,
                http_body_util::Either::Right(http_body_util::Full::new(rewritten)),
            ))
        }))
    }
}
