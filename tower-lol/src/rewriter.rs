//! TODO

use crate::util::{EitherError, UnsafeSend};
use bytes::{Buf, Bytes, BytesMut};
use http::header::Entry;
use http::{Request, Response};
use http_body::Body;
use lol_html::errors::RewritingError;
use lol_html::{HtmlRewriter, Settings};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};

/// TODO
pub trait SettingsProvider {
    /// TODO
    fn set_request(&mut self, req: &http::request::Parts);
    /// TODO
    fn provide<'b, 'a: 'b>(
        &mut self,
        res: &'a mut http::response::Parts,
    ) -> Option<Settings<'b, 'static>>;
}

/// TODO
#[derive(Debug, Clone)]
pub struct HtmlRewriterLayer<C> {
    settings: C,
}

impl<C> HtmlRewriterLayer<C> {
    /// TODO
    pub fn new(settings: C) -> Self {
        Self { settings }
    }
}

impl<S, C: Clone> Layer<S> for HtmlRewriterLayer<C> {
    type Service = HtmlRewriterService<C, S>;

    fn layer(&self, inner: S) -> Self::Service {
        Self::Service::new(inner, self.settings.clone())
    }
}

/// TODO
#[derive(Clone, Debug)]
pub struct HtmlRewriterService<C, S> {
    settings: C,
    inner: S,
}

impl<C, S> HtmlRewriterService<C, S> {
    /// TODO
    pub fn new(inner: S, settings: C) -> Self {
        Self { inner, settings }
    }
}

impl<S, C, ReqBody, ResBody> Service<Request<ReqBody>> for HtmlRewriterService<C, S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    C: SettingsProvider + Clone,
    ResBody: Body + Unpin,
    ResBody::Error: Error + Send + Sync + 'static,
{
    type Response = Response<HtmlRewriterBody<ResBody, RewritingError>>;
    type Error = S::Error;
    type Future = impl Future<Output = Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut cloned = std::mem::replace(self, self.clone());

        Box::pin(async move {
            let req = {
                let (parts, body) = req.into_parts();
                cloned.settings.set_request(&parts);
                Request::from_parts(parts, body)
            };

            let (mut parts, body) = {
                let res = cloned.inner.call(req).await?;
                res.into_parts()
            };

            let body = match provide_settings(&mut cloned.settings, &mut parts) {
                Some(settings) => HtmlRewriterBody::new(body, settings).await,
                None => HtmlRewriterBody::passthrough(body),
            };

            if let Entry::Occupied(mut entry) = parts.headers.entry(http::header::CONTENT_LENGTH) {
                body.len().map(|len| entry.insert(len.into()));
            }

            Ok(Response::from_parts(parts, body))
        })
    }
}

pin_project_lite::pin_project! {
    #[doc(hidden)]
    pub struct HtmlRewriterBody<B, E> {
        #[pin]
        inner: B,
        error: Option<E>,
        rewritten: Option<Option<Bytes>>,
    }
}

impl<B> HtmlRewriterBody<B, RewritingError>
where
    B: http_body::Body + Unpin,
    B::Error: Error + Send + Sync + 'static,
{
    async fn new<'a>(mut body: B, settings: UnsafeSend<Settings<'a, 'static>>) -> Self {
        match handle_rewrite(settings, Pin::new(&mut body)).await {
            Ok(rewritten) => Self {
                inner: body,
                rewritten: Some(Some(rewritten.into())),
                error: None,
            },
            Err(error) => Self {
                inner: body,
                rewritten: None,
                error: Some(error),
            },
        }
    }
}

impl<B, E> HtmlRewriterBody<B, E> {
    fn passthrough(body: B) -> Self {
        Self {
            inner: body,
            error: None,
            rewritten: None,
        }
    }

    fn len(&self) -> Option<usize> {
        Some(self.rewritten.as_ref()?.as_ref()?.len())
    }
}

impl<B, E> http_body::Body for HtmlRewriterBody<B, E>
where
    B: http_body::Body,
{
    type Data = Bytes;
    type Error = EitherError<E, B::Error>;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        if let Some(err) = this.error.take() {
            return Poll::Ready(Some(Err(EitherError::A(err))));
        }

        if let Some(rewritten) = this.rewritten.as_mut().map(|it| it.take()) {
            return Poll::Ready(Ok(rewritten).transpose());
        }

        this.inner
            .poll_data(cx)
            .map_ok(|mut chunk| chunk.copy_to_bytes(chunk.remaining()))
            .map_err(EitherError::B)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let this = self.project();
        this.inner.poll_trailers(cx).map_err(EitherError::B)
    }
}

async fn handle_rewrite<'a, B>(
    settings: UnsafeSend<Settings<'a, 'static>>,
    mut body: Pin<&mut B>,
) -> Result<BytesMut, RewritingError>
where
    B: http_body::Body,
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
