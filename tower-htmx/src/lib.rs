//! Middlewares for building sites using [`htmx`] and [`tower`].
//!
//! [`htmx`]: https://htmx.org/reference/

#![forbid(unsafe_code, unused_unsafe)]
#![warn(clippy::all, missing_docs, nonstandard_style, future_incompatible)]
#![allow(clippy::type_complexity)]

mod presets;

use http::{Request, Response};
use http_body::Body;
use presets::{ExtractSettings, InsertSettings, SubsetSettings};
use std::error::Error;
use tower::{Layer, Service};
use tower_lol::resolve::ResolveService;
use tower_lol::rewriter::HtmlRewriterService;

/// Layer to apply [`TemplateService`] middleware.
#[derive(Debug, Clone)]
pub struct TemplateLayer {
    attribute_name: String,
}

impl TemplateLayer {
    /// Create a new [`TemplateLayer`].
    pub fn new() -> Self {
        Self {
            attribute_name: "hx-get".to_owned(),
        }
    }

    /// Set a custom attribute name for extracting the target part.
    pub fn attribute<T: Into<String>>(self, attribute_name: T) -> Self {
        #[allow(clippy::needless_update)]
        Self {
            attribute_name: attribute_name.into(),
            ..self
        }
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
        TemplateService::new(inner, self.attribute_name.clone())
    }
}

type InnerTemplateService<S> =
    HtmlRewriterService<InsertSettings, ResolveService<HtmlRewriterService<ExtractSettings, S>>>;

/// Middleware that templates a HTML document.
#[derive(Debug, Clone)]
pub struct TemplateService<S> {
    inner: InnerTemplateService<S>,
}

impl<S> TemplateService<S> {
    /// Create a new [`TemplateService`] middleware.
    pub fn new(inner: S, attribute_name: String) -> Self {
        let extract_svc =
            HtmlRewriterService::new(inner, ExtractSettings::new(attribute_name.clone()));
        let resolve_svc = ResolveService::new(extract_svc);
        let inject_svc = HtmlRewriterService::new(resolve_svc, InsertSettings::new(attribute_name));

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

/// Layer to apply [`SubsetService`] middleware.
#[derive(Debug, Clone)]
pub struct SubsetLayer {
    query_name: String,
}

impl SubsetLayer {
    /// Create a new [`SubsetLayer`].
    pub fn new() -> Self {
        Self {
            query_name: "hx-select".to_owned(),
        }
    }

    /// Set a custom query key for extracting the CSS selector.
    pub fn query<T: Into<String>>(self, query_name: T) -> Self {
        #[allow(clippy::needless_update)]
        Self {
            query_name: query_name.into(),
            ..self
        }
    }
}

impl Default for SubsetLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for SubsetLayer {
    type Service = SubsetService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SubsetService::new(inner, self.query_name.clone())
    }
}

type InnerSubsetService<S> = HtmlRewriterService<SubsetSettings, S>;

/// Middleware that selects a subset of HTML based on a query.
#[derive(Debug, Clone)]
pub struct SubsetService<S> {
    inner: InnerSubsetService<S>,
}

impl<S> SubsetService<S> {
    /// Create a new [`SubsetService`] middleware.
    pub fn new(inner: S, attribute_name: String) -> Self {
        let subset_svc = HtmlRewriterService::new(inner, SubsetSettings::new(attribute_name));

        Self { inner: subset_svc }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for SubsetService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    ReqBody: Body + Default,
    ResBody: Body + Unpin,
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
