//! Middleware that modifies HTML in-flight.

use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes, BytesMut};
use http::header::Entry;
use http::{Request, Response};
use http_body::{Body, Frame};
use lol_html::errors::RewritingError;
use lol_html::{HtmlRewriter, Settings};
use tower::{Layer, Service};

use crate::util::{EitherError, UnsafeSend};

/// Trait that provides [`Settings`] values to middlewares.
///
/// The structure of this trait makes it possible for settings to be based on
/// both http request and response metadata.
///
/// Additionally, the resulting settings may have mutable access to the provided
/// response metdata.  This makes it possible for settings callbacks to modify
/// said response metadata in addition to the response body.
pub trait SettingsProvider {
    /// Handle http request metadata by storing any required data.
    fn handle_request(&mut self, req: &http::request::Parts);
    /// Handle http response metadata by returning some dependent settings.
    fn handle_response<'b, 'a: 'b>(
        &mut self,
        res: &'a mut http::response::Parts,
    ) -> Option<Settings<'b, 'static>>;
}

/// Layer to apply [`HtmlRewriteService`] middleware.
#[derive(Debug, Clone)]
pub struct HtmlRewriteLayer<C> {
    settings: C,
}

impl<C> HtmlRewriteLayer<C> {
    /// Create a new [`HtmlRewriteLayer`].
    pub fn new(settings: C) -> Self {
        Self { settings }
    }
}

impl<S, C: Clone> Layer<S> for HtmlRewriteLayer<C> {
    type Service = HtmlRewriteService<C, S>;

    fn layer(&self, inner: S) -> Self::Service {
        Self::Service::new(inner, self.settings.clone())
    }
}

/// Middleware that modifies HTML in-flight.
#[derive(Clone, Debug)]
pub struct HtmlRewriteService<C, S> {
    settings: C,
    inner: S,
}

impl<C, S> HtmlRewriteService<C, S> {
    /// Create a new [`HtmlRewriteService`] middleware.
    pub fn new(inner: S, settings: C) -> Self {
        Self { inner, settings }
    }
}

impl<S, C, ReqBody, ResBody> Service<Request<ReqBody>> for HtmlRewriteService<C, S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    C: SettingsProvider + Clone,
    ResBody: Body + Unpin,
    ResBody::Error: Error + Send + Sync + 'static,
{
    type Response = Response<HtmlRewriteBody<ResBody, RewritingError>>;
    type Error = S::Error;
    type Future = impl Future<Output = Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut cloned = std::mem::replace(self, self.clone());

        async move {
            let req = {
                let (parts, body) = req.into_parts();
                cloned.settings.handle_request(&parts);
                Request::from_parts(parts, body)
            };

            let (mut parts, body) = {
                let res = cloned.inner.call(req).await?;
                res.into_parts()
            };

            let body = match provide_settings(&mut cloned.settings, &mut parts) {
                Some(settings) => HtmlRewriteBody::new(body).try_rewrite(settings).await,
                None => HtmlRewriteBody::new(body),
            };

            if let Entry::Occupied(mut entry) = parts.headers.entry(http::header::CONTENT_LENGTH) {
                body.len().map(|len| entry.insert(len.into()));
            }

            Ok(Response::from_parts(parts, body))
        }
    }
}

pin_project_lite::pin_project! {
    #[doc(hidden)]
    #[project = HtmlRewriteBodyProj]
    pub enum HtmlRewriteBody<B, E> {
        Normal { #[pin] inner: B },
        Rewritten { rewritten: Option<Bytes> },
        Err { error: Option<E> }
    }
}

impl<B, E> HtmlRewriteBody<B, E> {
    fn new(body: B) -> Self {
        Self::Normal { inner: body }
    }

    fn len(&self) -> Option<usize> {
        match self {
            Self::Rewritten { rewritten } => rewritten.as_ref().map(|data| data.len()),
            _ => None,
        }
    }
}

impl<B> HtmlRewriteBody<B, RewritingError>
where
    B: http_body::Body + Unpin,
    B::Error: Error + Send + Sync + 'static,
{
    async fn try_rewrite<'a>(self, settings: UnsafeSend<Settings<'a, 'static>>) -> Self {
        let mut body = match self {
            HtmlRewriteBody::Normal { inner } => inner,
            _ => return self,
        };

        match handle_rewrite(settings, Pin::new(&mut body)) {
            Ok(rewritten) => Self::Rewritten {
                rewritten: Some(rewritten.into()),
            },
            Err(error) => Self::Err { error: Some(error) },
        }
    }
}

impl<B, E> http_body::Body for HtmlRewriteBody<B, E>
where
    B: http_body::Body,
{
    type Data = Bytes;
    type Error = EitherError<E, B::Error>;

    // fn poll_data(
    //     self: Pin<&mut Self>,
    //     cx: &mut Context<'_>,
    // ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
    //     let this = self.project();
    //     match this {
    //         HtmlRewriteBodyProj::Normal { inner } => inner
    //             .poll_data(cx)
    //             .map_ok(|mut chunk| chunk.copy_to_bytes(chunk.remaining()))
    //             .map_err(EitherError::B),
    //         HtmlRewriteBodyProj::Rewritten { rewritten } => {
    //             Poll::Ready(Ok(rewritten.take()).transpose())
    //         }
    //         HtmlRewriteBodyProj::Err { error } => match error.take() {
    //             Some(error) => Poll::Ready(Some(Err(EitherError::A(error)))),
    //             None => Poll::Ready(None),
    //         },
    //     }
    // }

    // fn poll_trailers(
    //     self: Pin<&mut Self>,
    //     cx: &mut Context<'_>,
    // ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
    //     let this = self.project();
    //     if let HtmlRewriteBodyProj::Normal { inner } = this {
    //         inner.poll_trailers(cx).map_err(EitherError::B)
    //     } else {
    //         Poll::Ready(Ok(None))
    //     }
    // }

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        match this {
            HtmlRewriteBodyProj::Normal { inner } => inner
                .poll_frame(cx)
                .map_ok(|frame| frame.map_data(|mut chunk| chunk.copy_to_bytes(chunk.remaining())))
                .map_err(EitherError::B),
            HtmlRewriteBodyProj::Rewritten { rewritten } => {
                Poll::Ready(rewritten.take().map(|it| Ok(Frame::data(it))))
            }
            HtmlRewriteBodyProj::Err { error } => {
                Poll::Ready(error.take().map(|err| Err(EitherError::A(err))))
            }
        }
    }
}

fn handle_rewrite<'a, B>(
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

    // while let Some(chunk) = body
    //     .poll_frame(cx)
    //     .map_err(|err| RewritingError::ContentHandlerError(Box::new(err)))?
    // {
    //     rewriter.inner.write(chunk.chunk())?
    // }

    rewriter.inner.end()?;

    Ok(output)
}

fn provide_settings<'b, 'a: 'b, S: SettingsProvider>(
    settings: &mut S,
    res: &'a mut http::response::Parts,
) -> Option<UnsafeSend<Settings<'b, 'static>>> {
    settings
        .handle_response(res)
        .map(|it| unsafe { UnsafeSend::new(it) })
}
