use std::{
    future::Future,
    pin::Pin,
    task::{ready, Poll},
};

use bytes::{Buf, Bytes, BytesMut};
use http::{Request, Response};
use tower::{Layer, Service};

pub struct MultiContext<T> {
    entries: std::collections::HashMap<String, T>,
}

#[derive(Debug, Clone)]
pub struct HoldupLayer {}

impl HoldupLayer {
    pub fn new() -> Self {
        Self {}
    }
}

impl<S> Layer<S> for HoldupLayer {
    type Service = HoldupService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HoldupService::new(inner)
    }
}

#[derive(Debug, Clone)]
pub struct HoldupService<S> {
    inner: S,
}

impl<S> HoldupService<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for HoldupService<S>
where
    S: Service<Request<ReqBody>>,
    S::Future: Future<Output = Result<Response<ResBody>, S::Error>>,
    ResBody: http_body::Body + Unpin,
{
    type Response = Response<HoldupBody<ResBody>>;
    type Error = S::Error;
    type Future = HoldupFuture<ResBody, S::Error, S::Future>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        HoldupFuture {
            inner: self.inner.call(req),
            buffer: Some(BytesMut::new()),
            response: None,
            _phantom: Default::default(),
        }
    }
}

pin_project_lite::pin_project! {
    pub struct HoldupFuture<B, E, F> {
        #[pin]
        inner: F,
        buffer: Option<BytesMut>,
        response: Option<Response<B>>,
        _phantom: std::marker::PhantomData<E>,
    }
}

impl<B, E, F> Future for HoldupFuture<B, E, F>
where
    F: Future<Output = Result<Response<B>, E>>,
    B: http_body::Body + Unpin,
{
    type Output = Result<Response<HoldupBody<B>>, E>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let mut response = if let Some(response) = this.response.take() {
            response
        } else {
            *this.response = Some(ready!(this.inner.poll(cx)?));
            return Poll::Pending;
        };

        response.extensions_mut().remove::<MultiContext<String>>();

        Poll::Ready(Ok(response.map(|b| HoldupBody::new(b))))

        // match ready!(Pin::new(response.body_mut()).poll_data(cx)) {
        //     Some(Ok(mut chunk)) => {
        //         this.buffer
        //             .as_mut()
        //             .map(|buf|
        // buf.extend(chunk.copy_to_bytes(chunk.remaining())));

        //         Poll::Pending
        //     }
        //     Some(Err(err)) => {
        //         todo!()
        //     }
        //     None => Poll::Ready(Ok(this
        //         .response
        //         .take()
        //         .unwrap()
        //         .map(|_| this.buffer.take().expect("TODO").into()))),
        // }
    }
}

pin_project_lite::pin_project! {
    pub struct HoldupBody<B> {
        #[pin]
        body: B,
    }
}

impl<B> HoldupBody<B> {
    fn new(body: B) -> Self {
        Self { body }
    }
}

impl<B: http_body::Body> http_body::Body for HoldupBody<B> {
    type Data = Bytes;
    type Error = B::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        this.body
            .poll_data(cx)
            .map_ok(|mut chunk| chunk.copy_to_bytes(chunk.remaining()))
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let this = self.project();
        this.body.poll_trailers(cx)
    }
}
