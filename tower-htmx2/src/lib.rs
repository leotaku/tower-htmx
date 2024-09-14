#![feature(impl_trait_in_assoc_type)]
#![forbid(unused_unsafe)]
#![warn(clippy::all, missing_docs, nonstandard_style, future_incompatible)]
#![allow(clippy::type_complexity)]

//! Foo

mod future;
mod sync;

use std::future::{poll_fn, Future};
use std::mem;

use bytes::Bytes;
use http::{Request, Response, Uri};
use http_body_util::BodyExt;
use sync::{rewrite, scan_tags};
use tower::{Layer, Service};

#[derive(Debug, Clone)]
pub struct HtmxRewriteLayer;

impl HtmxRewriteLayer {
    pub fn new() -> Self {
        HtmxRewriteLayer
    }
}

impl<S> Layer<S> for HtmxRewriteLayer {
    type Service = HtmxRewriteService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HtmxRewriteService::new(inner)
    }
}

#[derive(Debug, Clone)]
pub struct HtmxRewriteService<S>(S);

impl<S> HtmxRewriteService<S> {
    fn new(inner: S) -> Self {
        HtmxRewriteService(inner)
    }
}

impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for HtmxRewriteService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone,
    ReqBody: Default,
    ResBody: http_body::Body<Data = Bytes>,
{
    type Response = Response<http_body_util::Either<ResBody, http_body_util::Full<Bytes>>>;
    type Error = Error<S::Error, ResBody::Error>;
    type Future = impl Future<Output = Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0
            .poll_ready(cx)
            .map_err(|err| Error::Future(err.into()))
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        if req.headers().contains_key(http::header::CONTENT_LENGTH)
            && req
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|it| it.to_str().ok())
                .is_some_and(|it| it.starts_with("text/html"))
        {
            let fut = self.0.call(req);
            return future::Either::left(async move {
                let rsp = fut.await.map_err(Error::Future)?;
                Ok(rsp.map(http_body_util::Either::Left))
            });
        };

        let uri = req.uri().clone();
        let recursion_level = match req.extensions().get::<RecursionProtector>() {
            Some(RecursionProtector(level)) => *level,
            None => 0,
        };

        let fut = self.0.call(req);
        let service = mem::replace(self, self.clone());

        future::Either::right(Box::pin(async move {
            let res = fut.await.map_err(Error::Future)?;
            rewrite_call(&uri, res, service, recursion_level).await
        }))
    }
}

#[derive(Debug)]
pub enum Error<F, B> {
    Future(F),
    Body(B),
    HTTP(http::Error),
    Recursion,
}

impl<F, B> Error<F, B> {
    pub fn to_html(self) -> String {
        match self {
            Error::Future(err) => "future error".to_owned(),
            Error::Body(err) => "body error".to_owned(),
            Error::HTTP(err) => "http error".to_owned(),
            Error::Recursion => "recursion error".to_owned(),
        }
    }
}

async fn rewrite_call<S, ReqBody, ResBody>(
    uri: &http::uri::Uri,
    res: Response<ResBody>,
    mut service: HtmxRewriteService<S>,
    recursion_level: usize,
) -> Result<
    <HtmxRewriteService<S> as Service<http::Request<ReqBody>>>::Response,
    Error<S::Error, ResBody::Error>,
>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone,
    ReqBody: Default,
    ResBody: http_body::Body<Data = Bytes>,
{
    if recursion_level > 10 {
        return Err(Error::Recursion);
    }

    let (mut parts, body) = res.into_parts();
    let original = body.collect().await.map_err(Error::Body)?.to_bytes();
    let hrefs = scan_tags(&original).unwrap();

    let mut resps = Vec::new();
    for href in hrefs {
        poll_fn(|cx| service.poll_ready(cx)).await?;
        let req = Request::builder()
            .method(http::method::Method::GET)
            .uri(expand_uri(&uri, &href).map_err(Error::HTTP)?)
            .extension(RecursionProtector(recursion_level))
            .body(ReqBody::default())
            .unwrap();

        let (parts, body) = service.call(req).await?.into_parts();
        let body = match body {
            http_body_util::Either::Left(left) => left.collect().await.map_err(Error::Body)?,
            http_body_util::Either::Right(right) => right.collect().await.expect("fuck off"),
        };

        resps.push(Response::from_parts(parts, body));
    }

    let rewritten = rewrite::<ResBody::Data>(&original, resps).unwrap();

    parts
        .headers
        .insert(http::header::CONTENT_LENGTH, rewritten.len().into());

    Ok(http::Response::from_parts(
        parts,
        http_body_util::Either::Right(http_body_util::Full::new(rewritten)),
    ))
}

fn expand_uri(uri: &http::uri::Uri, path: &str) -> http::Result<http::uri::Uri> {
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = if path.starts_with("/") {
        Some(path.try_into()?)
    } else {
        Some(format!("{}/{}", uri.path(), path).try_into()?)
    };

    Ok(Uri::from_parts(parts)?)
}

#[derive(Debug, Clone)]
struct RecursionProtector(usize);
