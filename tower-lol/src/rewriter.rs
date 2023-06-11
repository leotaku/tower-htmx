use crate::either::{EitherError, StoringFuture};

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
    task::{ready, Context, Poll},
};

use bytes::{Buf, Bytes};
use http::{Request, Response};
use lol_html::{errors::RewritingError, HtmlRewriter, Settings};
use tower::{Layer, Service};

pub struct HtmlRewriterLayer<Sett> {
    settings: Sett,
}

pub trait SettingsProvider<B>: Send + Sync + Clone {
    type Future: Future<Output = Option<Settings<'static, 'static>>>;

    fn provide(&self, req: &Request<B>) -> Self::Future;
}

impl<F, Fut, B> SettingsProvider<B> for F
where
    F: Fn(&Request<B>) -> Fut + Send + Sync + Clone,
    Fut: Future<Output = Option<Settings<'static, 'static>>>,
{
    type Future = Fut;

    fn provide(&self, req: &Request<B>) -> Self::Future {
        self(req)
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

impl<S, Sett, ReqBody, ResBody> Service<Request<ReqBody>> for HtmlRewriterService<S, Sett>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: http_body::Body,
    Sett: SettingsProvider<ReqBody>,
{
    type Response = Response<HtmlRewriterBody<ResBody>>;
    type Error = S::Error;
    type Future = HtmlRewriterFuture<S::Future, Sett::Future, <S::Future as Future>::Output>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let settings = self.settings.provide(&request);
        HtmlRewriterFuture {
            inner: StoringFuture::new(self.inner.call(request)),
            settings,
        }
    }
}

pin_project_lite::pin_project! {
    pub struct HtmlRewriterFuture<F, Sett, FO> {
        #[pin]
        inner: StoringFuture<FO, F>,
        #[pin]
        settings: Sett,
    }
}

impl<PB, PE, F, Sett> Future for HtmlRewriterFuture<F, Sett, Result<Response<PB>, PE>>
where
    F: Future<Output = Result<Response<PB>, PE>>,
    Sett: Future<Output = Option<Settings<'static, 'static>>>,
{
    type Output = Result<Response<HtmlRewriterBody<PB>>, PE>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        ready!(this.inner.as_mut().poll(cx));
        let settings = ready!(this.settings.poll(cx));
        let response = this.inner.take()?;

        let (parts, body) = response.into_parts();

        let new_body = match settings {
            Some(settings) => HtmlRewriterBody::new(body, settings),
            None => HtmlRewriterBody::passthrough(body),
        };

        Poll::Ready(Ok(Response::from_parts(parts, new_body)))
    }
}

pin_project_lite::pin_project! {
    pub struct HtmlRewriterBody<B> {
        #[pin]
        body: B,
        rewriter: Option<UnsafeSend<HtmlRewriter<'static, Sink>>>,
        sink: Sink,
    }
}

impl<B> HtmlRewriterBody<B> {
    fn new(body: B, settings: Settings<'static, 'static>) -> Self {
        let sink = Sink::new();
        Self {
            body,
            rewriter: Some(unsafe { UnsafeSend::new(HtmlRewriter::new(settings, sink.clone())) }),
            sink,
        }
    }

    fn passthrough(body: B) -> Self {
        Self {
            body,
            rewriter: None,
            sink: Sink::new(),
        }
    }
}

impl<B: http_body::Body> http_body::Body for HtmlRewriterBody<B> {
    type Data = Bytes;
    type Error = EitherError<B::Error, RewritingError>;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        println!("poll_data");
        let this = self.project();
        let chunk = ready!(this
            .body
            .poll_data(cx)
            .map_ok(|mut chunk| chunk.copy_to_bytes(chunk.remaining()))
            .map_err(EitherError::A)?);

        if this.rewriter.is_none() {
            return Poll::Ready(Ok(chunk).transpose());
        }

        if let Some(chunk) = chunk {
            this.rewriter
                .as_mut()
                .unwrap()
                .0
                .write(chunk.as_ref())
                .map_err(EitherError::B)?;
        } else if let Some(rewriter) = this.rewriter.take() {
            rewriter.0.end().map_err(EitherError::B)?;
        }

        if let Some(chunk) = this.sink.pop() {
            Poll::Ready(Some(Ok(chunk)))
        } else if this.rewriter.is_some() {
            Poll::Pending
        } else {
            Poll::Ready(None)
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        self.project()
            .body
            .poll_trailers(cx)
            .map(|it| it.map_err(EitherError::A))
    }
}

#[derive(Clone)]
struct Sink {
    chunk_buffer: Arc<RwLock<Vec<Bytes>>>,
}

impl Sink {
    fn new() -> Self {
        Self {
            chunk_buffer: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn pop(&self) -> Option<Bytes> {
        if let Ok(mut buffer) = self.chunk_buffer.try_write() {
            buffer.pop()
        } else {
            None
        }
    }
}

impl lol_html::OutputSink for Sink {
    fn handle_chunk(&mut self, chunk: &[u8]) {
        if let Ok(mut buffer) = self.chunk_buffer.try_write() {
            buffer.push(Bytes::copy_from_slice(chunk));
        }
    }
}

struct UnsafeSend<T>(T);

impl<T> UnsafeSend<T> {
    unsafe fn new(value: T) -> Self {
        Self(value)
    }
}

unsafe impl<T> Send for UnsafeSend<T> {}
