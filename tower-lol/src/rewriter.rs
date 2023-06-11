use crate::{
    either::EitherBody,
    util::{ErrorBody, UnsafeSend},
};

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes, BytesMut};
use http::{header::Entry, uri::PathAndQuery, Request, Response};
use http_body::{Body, Full};
use lol_html::{errors::RewritingError, HtmlRewriter, Settings};
use tower::{Layer, Service};

pub struct HtmlRewriterLayer<Sett> {
    settings: Sett,
}

pub trait SettingsProvider<ReqBody, ResBody> {
    fn set_request(&mut self, req: &Request<ReqBody>);
    fn provide<'b, 'a: 'b>(
        &mut self,
        res: &'a mut Response<ResBody>,
    ) -> Option<Settings<'b, 'static>>;
}

fn provide_settings<'b, 'a: 'b, ReqBody, ResBody, S: SettingsProvider<ReqBody, ResBody>>(
    settings: &mut S,
    res: &'a mut Response<ResBody>,
) -> Option<UnsafeSend<Settings<'b, 'static>>> {
    settings
        .provide(res)
        .map(|it| unsafe { UnsafeSend::new(it) })
}

#[derive(Debug, Clone)]
pub struct SettingsFromQuery<F> {
    query: Option<PathAndQuery>,
    function: F,
}

impl<F> SettingsFromQuery<F> {
    pub fn new(function: F) -> Self {
        Self {
            query: None,
            function,
        }
    }
}

impl<F, ReqBody, ResBody> SettingsProvider<ReqBody, ResBody> for SettingsFromQuery<F>
where
    F: Fn(&PathAndQuery, &mut Response<ResBody>) -> Option<Settings<'static, 'static>> + Copy,
{
    fn set_request(&mut self, req: &Request<ReqBody>) {
        self.query = req.uri().path_and_query().cloned();
    }

    fn provide<'b, 'a: 'b>(
        &mut self,
        res: &'a mut Response<ResBody>,
    ) -> Option<Settings<'b, 'static>> {
        let f = self.function;
        f(self.query.as_mut().unwrap(), res)
    }
}

impl<Sett> HtmlRewriterLayer<Sett> {
    pub fn new(settings: Sett) -> Self {
        Self { settings }
    }
}

impl<Sett: Clone> Clone for HtmlRewriterLayer<Sett> {
    fn clone(&self) -> Self {
        Self {
            settings: self.settings.clone(),
        }
    }
}

impl<S, Sett: Clone> Layer<S> for HtmlRewriterLayer<Sett> {
    type Service = HtmlRewriterService<S, Sett>;

    fn layer(&self, inner: S) -> Self::Service {
        Self::Service::new(inner, self.settings.clone())
    }
}

pub struct HtmlRewriterService<S, Sett> {
    inner: S,
    settings: Sett,
}

impl<S: Clone, Sett: Clone> Clone for HtmlRewriterService<S, Sett> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            settings: self.settings.clone(),
        }
    }
}

impl<S, Sett> HtmlRewriterService<S, Sett> {
    pub fn new(inner: S, settings: Sett) -> Self {
        Self { inner, settings }
    }
}

type TriBody<A, B, C> = EitherBody<A, EitherBody<B, C>>;

impl<S, Sett, ReqBody, ResBody> Service<Request<ReqBody>> for HtmlRewriterService<S, Sett>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    Sett: SettingsProvider<ReqBody, ResBody> + Clone + Send + 'static,
    ResBody: Body + Unpin + Send,
    ReqBody: Send + 'static,
{
    type Response = Response<TriBody<Full<Bytes>, ResBody, ErrorBody<Bytes, RewritingError>>>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut cloned = self.clone();

        Box::pin(async move {
            std::future::poll_fn(|cx| cloned.poll_ready(cx)).await?;
            cloned.settings.set_request(&req);

            let mut response: Response<_> = cloned.inner.call(req).await?;

            let settings = match provide_settings(&mut cloned.settings, &mut response) {
                Some(settings) => settings,
                None => return Ok(response.map(|body| EitherBody::b(EitherBody::a(body)))),
            };

            let (mut parts, body) = response.into_parts();
            let output = match handle_rewrite(settings, body).await {
                Ok(output) => output,
                Err(error) => {
                    return Ok(Response::from_parts(
                        parts,
                        EitherBody::b(EitherBody::b(ErrorBody::new(error))),
                    ))
                }
            };

            if let Entry::Occupied(mut entry) = parts.headers.entry(http::header::CONTENT_LENGTH) {
                entry.insert(output.len().into());
            }

            Ok(Response::from_parts(
                parts,
                EitherBody::a(Full::new(output.into())),
            ))
        })
    }
}

async fn handle_rewrite<B: http_body::Body + Unpin>(
    settings: UnsafeSend<Settings<'static, 'static>>,
    mut body: B,
) -> Result<BytesMut, RewritingError> {
    let mut output = BytesMut::new();
    let mut rewriter = unsafe {
        UnsafeSend::new(HtmlRewriter::new(settings.inner, |chunk: &[u8]| {
            output.extend_from_slice(chunk)
        }))
    };

    while let Some(chunk) = body.data().await.transpose().unwrap_or_else(|_| panic!()) {
        rewriter.inner.write(chunk.chunk())?
    }

    rewriter.inner.end()?;

    Ok(output)
}
