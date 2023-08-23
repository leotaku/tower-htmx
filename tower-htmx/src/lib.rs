//! Middlewares for building sites using [`htmx`] and [`tower`].
//!
//! [`htmx`]: https://htmx.org/reference/

#![forbid(unsafe_code, unused_unsafe)]
#![warn(clippy::all, missing_docs, nonstandard_style, future_incompatible)]
#![allow(clippy::type_complexity)]

mod presets;

use std::error::Error;

use http::{Request, Response};
use http_body::Body;
use tower::{Layer, Service};
use tower_lol::resolve::ResolveService;
use tower_lol::rewrite::HtmlRewriteService;

use crate::presets::{ExtractSettings, InsertSettings, SelectSettings};

/// Layer to apply [`TemplateService`] middleware.
#[derive(Debug, Clone)]
pub struct TemplateLayer {}

impl TemplateLayer {
    /// Create a new [`TemplateLayer`].
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for TemplateLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for TemplateLayer {
    type Service = TemplateService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TemplateService::new(inner)
    }
}

type InnerTemplateService<S> =
    HtmlRewriteService<InsertSettings, ResolveService<HtmlRewriteService<ExtractSettings, S>>>;

/// Middleware that templates a HTML document.
#[derive(Debug, Clone)]
pub struct TemplateService<S> {
    inner: InnerTemplateService<S>,
}

impl<S> TemplateService<S> {
    /// Create a new [`TemplateService`] middleware.
    pub fn new(inner: S) -> Self {
        let extract_svc = HtmlRewriteService::new(inner, ExtractSettings::new());
        let resolve_svc = ResolveService::new(extract_svc);
        let inject_svc = HtmlRewriteService::new(resolve_svc, InsertSettings::new());

        Self { inner: inject_svc }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for TemplateService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Body + Default,
    ResBody: Body + Unpin,
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

/// Layer to apply [`SelectService`] middleware.
#[derive(Debug, Clone)]
pub struct SelectLayer {}

impl SelectLayer {
    /// Create a new [`SelectLayer`].
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for SelectLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for SelectLayer {
    type Service = SelectService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SelectService::new(inner)
    }
}

type InnerSelectService<S> = HtmlRewriteService<SelectSettings, S>;

/// Middleware that selects a subset of HTML based on a query.
#[derive(Debug, Clone)]
pub struct SelectService<S> {
    inner: InnerSelectService<S>,
}

impl<S> SelectService<S> {
    /// Create a new [`SelectService`] middleware.
    pub fn new(inner: S) -> Self {
        let subset_svc = HtmlRewriteService::new(inner, SelectSettings::new());

        Self { inner: subset_svc }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for SelectService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Body + Default,
    ResBody: Body + Unpin,
    ResBody::Error: Error + Send + Sync + 'static,
{
    type Response = <InnerSelectService<S> as Service<Request<ReqBody>>>::Response;
    type Error = <InnerSelectService<S> as Service<Request<ReqBody>>>::Error;
    type Future = <InnerSelectService<S> as Service<Request<ReqBody>>>::Future;

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
