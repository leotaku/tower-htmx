//! TODO

#![forbid(unused_unsafe)]
#![warn(clippy::all, missing_docs, nonstandard_style, future_incompatible)]
#![allow(clippy::type_complexity)]

mod either;
mod presets;
mod resolve;
mod rewriter;
mod util;

use std::error::Error;

use bytes::Bytes;
use http::{Request, Response};
use http_body::Body;
use presets::{ExtractSettings, InsertSettings, SubsetSettings};
use resolve::ResolveService;
use rewriter::HtmlRewriterService;
use tower::{Layer, Service};

/// TODO
#[derive(Debug, Clone)]
pub struct TemplateLayer {}

impl TemplateLayer {
    /// TODO
    pub fn new() -> Self {
        Self {}
    }
}

impl<S> Layer<S> for TemplateLayer {
    type Service = TemplateService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TemplateService::new(inner)
    }
}

type InnerTemplateService<S> =
    HtmlRewriterService<InsertSettings, ResolveService<HtmlRewriterService<ExtractSettings, S>>>;

/// TODO
#[derive(Debug, Clone)]
pub struct TemplateService<S> {
    inner: InnerTemplateService<S>,
}

impl<S> TemplateService<S> {
    /// TODO
    pub fn new(inner: S) -> Self {
        let extract_svc = HtmlRewriterService::new(inner, ExtractSettings::new());
        let resolve_svc = ResolveService::new(extract_svc);
        let inject_svc = HtmlRewriterService::new(resolve_svc, InsertSettings::new());

        Self { inner: inject_svc }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for TemplateService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    ReqBody: Body + Default + Send + 'static,
    ResBody: Body<Data = Bytes> + Unpin + Send,
    ResBody::Error: Error + Send + Sync + 'static,
{
    type Response = <InnerTemplateService<S> as Service<Request<ReqBody>>>::Response;
    type Error = <InnerTemplateService<S> as Service<Request<ReqBody>>>::Error;
    type Future = <InnerTemplateService<S> as Service<Request<ReqBody>>>::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.inner.call(req)
    }
}

/// TODO
#[derive(Debug, Clone)]
pub struct SubsetLayer {}

impl SubsetLayer {
    /// TODO
    pub fn new() -> Self {
        Self {}
    }
}

impl<S> Layer<S> for SubsetLayer {
    type Service = SubsetService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SubsetService::new(inner)
    }
}

type InnerSubsetService<S> = HtmlRewriterService<SubsetSettings, S>;

/// TODO
#[derive(Debug, Clone)]
pub struct SubsetService<S> {
    inner: InnerSubsetService<S>,
}

impl<S> SubsetService<S> {
    /// TODO
    pub fn new(inner: S) -> Self {
        let subset_svc = HtmlRewriterService::new(inner, SubsetSettings::new());

        Self { inner: subset_svc }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for SubsetService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    ReqBody: Body + Default + Send + 'static,
    ResBody: Body<Data = Bytes> + Unpin + Send,
    ResBody::Error: Error + Send + Sync + 'static,
{
    type Response = <InnerSubsetService<S> as Service<Request<ReqBody>>>::Response;
    type Error = <InnerSubsetService<S> as Service<Request<ReqBody>>>::Error;
    type Future = <InnerSubsetService<S> as Service<Request<ReqBody>>>::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        self.inner.call(req)
    }
}
