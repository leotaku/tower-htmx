use std::{
    fmt::Display,
    future::Future,
    sync::{Arc, RwLock},
    task::{ready, Poll},
};

use bytes::{Buf, Bytes};
use http::{Request, Response};
use lol_html::{errors::RewritingError, HtmlRewriter, Settings};
use tower::{Layer, Service};

type SettingsFn<'h, 's, ReqBody> = Arc<dyn Fn(&Request<ReqBody>) -> Settings<'h, 's> + Send + Sync>;

pub struct LolLayer<'h, 's, ReqBody> {
    settings: SettingsFn<'h, 's, ReqBody>,
}

impl<'h, 's, ReqBody> LolLayer<'h, 's, ReqBody> {
    pub fn new(
        settings: impl Fn(&Request<ReqBody>) -> Settings<'h, 's> + Send + Sync + 'static,
    ) -> Self {
        Self {
            settings: Arc::new(settings),
        }
    }
}

impl<'h, 's, ReqBody> Clone for LolLayer<'h, 's, ReqBody> {
    fn clone(&self) -> Self {
        Self {
            settings: self.settings.clone(),
        }
    }
}

impl<'h, 's, S, ReqBody> Layer<S> for LolLayer<'h, 's, ReqBody> {
    type Service = LolService<'h, 's, S, ReqBody>;

    fn layer(&self, inner: S) -> Self::Service {
        Self::Service::new_from_layer(inner, self.settings.clone())
    }
}

pub struct LolService<'h, 's, S, ReqBody> {
    inner: S,
    settings: SettingsFn<'h, 's, ReqBody>,
}

impl<'h, 's, S: Clone, ReqBody> Clone for LolService<'h, 's, S, ReqBody> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            settings: self.settings.clone(),
        }
    }
}

impl<'h, 's, 'c, S, ReqBody> LolService<'h, 's, S, ReqBody> {
    pub fn new(
        inner: S,
        settings: impl Fn(&Request<ReqBody>) -> Settings<'h, 's> + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner,
            settings: Arc::new(settings),
        }
    }

    fn new_from_layer(inner: S, settings: SettingsFn<'h, 's, ReqBody>) -> Self {
        Self { inner, settings }
    }
}

impl<'h, 's, 'c, S, ReqBody, ResBody> Service<Request<ReqBody>> for LolService<'h, 's, S, ReqBody>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: http_body::Body,
{
    type Response = Response<LolBody<'h, ResBody>>;
    type Error = S::Error;
    type Future = LolFuture<'h, 's, S::Future>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<ReqBody>) -> Self::Future {
        let settings = (self.settings)(&request);
        LolFuture {
            inner: self.inner.call(request),
            settings: Some(unsafe { UnsafeSend::new(settings) }),
        }
    }
}

pin_project_lite::pin_project! {
    pub struct LolFuture<'h, 's, F> {
        #[pin]
        inner: F,
        settings: Option<UnsafeSend<Settings<'h, 's>>>,
    }
}

impl<'h, 's, PB, PE, F> Future for LolFuture<'h, 's, F>
where
    F: Future<Output = Result<Response<PB>, PE>>,
{
    type Output = Result<Response<LolBody<'h, PB>>, PE>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let response = ready!(this.inner.poll(cx))?;

        let (parts, body) = response.into_parts();

        let new_body = LolBody::new(
            body,
            this.settings
                .take()
                .expect("poll to not be called on completed futures")
                .0
                .into_inner()
                .unwrap(),
        );

        Poll::Ready(Ok(Response::from_parts(parts, new_body)))
    }
}

pin_project_lite::pin_project! {
    pub struct LolBody<'h, B> {
        #[pin]
        body: B,
        rewriter: Option<UnsafeSend<HtmlRewriter<'h, Sink>>>,
        sink: Sink,
    }
}

impl<'h, B> LolBody<'h, B> {
    fn new<'s>(body: B, settings: Settings<'h, 's>) -> Self {
        let sink = Sink::new();
        Self {
            body,
            rewriter: Some(unsafe { UnsafeSend::new(HtmlRewriter::new(settings, sink.clone())) }),
            sink,
        }
    }
}

impl<'h, B: http_body::Body> http_body::Body for LolBody<'h, B> {
    type Data = Bytes;
    type Error = EitherError<B::Error, RewritingError>;

    fn poll_data(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        let poll = ready!(this
            .body
            .poll_data(cx)
            .map_ok(|mut chunk| chunk.copy_to_bytes(chunk.remaining()))?);

        if this.rewriter.is_none() {
            return Poll::Ready(Ok(poll).transpose());
        }

        if let Some(chunk) = poll {
            this.rewriter
                .as_mut()
                .map(|it| it.0.get_mut().map(|it| it.write(chunk.as_ref())).unwrap())
                .unwrap_or_else(|| Ok(()))
                .map_err(EitherError::B)?;
        } else if let Some(rewriter) = this.rewriter.take() {
            rewriter
                .0
                .into_inner()
                .unwrap()
                .end()
                .map_err(EitherError::B)?;
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
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
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

#[derive(Debug, Clone)]
pub enum EitherError<A, B> {
    A(A),
    B(B),
}

impl<A, B> From<A> for EitherError<A, B> {
    fn from(value: A) -> Self {
        EitherError::A(value)
    }
}

impl<A: std::error::Error, B: std::error::Error> std::error::Error for EitherError<A, B> {}

impl<A: Display, B: Display> Display for EitherError<A, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EitherError::A(a) => a.fmt(f),
            EitherError::B(b) => b.fmt(f),
        }
    }
}

struct UnsafeSend<T>(RwLock<T>);

impl<T> UnsafeSend<T> {
    unsafe fn new(value: T) -> Self {
        Self(RwLock::new(value))
    }
}

unsafe impl<T> Send for UnsafeSend<T> {}
