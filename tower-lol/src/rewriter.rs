use crate::{
    either::EitherBody,
    settings::SettingsProvider,
    util::{ErrorBody, UnsafeSend},
};

use std::{
    error::Error,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Buf, Bytes, BytesMut};
use http::{header::Entry, Request, Response};
use http_body::{Body, Full};
use lol_html::{errors::RewritingError, HtmlRewriter, Settings};
use tower::{Layer, Service};

pub struct HtmlRewriterLayer<Sett> {
    settings: Sett,
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
    Sett: SettingsProvider + Clone + Send + 'static,
    ReqBody: Send + 'static,
    ResBody: Body + Unpin + Send,
    ResBody::Error: Error + Send + Sync + 'static,
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
            let (parts, body) = req.into_parts();
            cloned.settings.set_request(&parts);
            let req = Request::from_parts(parts, body);

            let response: Response<_> = cloned.inner.call(req).await?;

            let (mut parts, body) = response.into_parts();
            let settings = match provide_settings(&mut cloned.settings, &mut parts) {
                Some(settings) => settings,
                none @ None => {
                    drop(none);
                    return Ok(Response::from_parts(
                        parts,
                        EitherBody::b(EitherBody::a(body)),
                    ));
                }
            };

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

async fn handle_rewrite<'a, B>(
    settings: UnsafeSend<Settings<'a, 'static>>,
    mut body: B,
) -> Result<BytesMut, RewritingError>
where
    B: http_body::Body + Unpin,
    B::Error: Error + Send + Sync + 'static,
{
    let mut output = BytesMut::new();
    let mut rewriter = unsafe {
        UnsafeSend::new(HtmlRewriter::new(settings.inner, |chunk: &[u8]| {
            output.extend_from_slice(chunk)
        }))
    };

    while let Some(chunk) = body
        .data()
        .await
        .transpose()
        .map_err(|err| RewritingError::ContentHandlerError(Box::new(err)))?
    {
        rewriter.inner.write(chunk.chunk())?
    }

    rewriter.inner.end()?;

    Ok(output)
}

fn provide_settings<'b, 'a: 'b, S: SettingsProvider>(
    settings: &mut S,
    res: &'a mut http::response::Parts,
) -> Option<UnsafeSend<Settings<'b, 'static>>> {
    settings
        .provide(res)
        .map(|it| unsafe { UnsafeSend::new(it) })
}
