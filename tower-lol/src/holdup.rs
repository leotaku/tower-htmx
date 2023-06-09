use std::{
    future::Future,
    pin::Pin,
    task::{ready, Poll},
};

use bytes::{Buf, Bytes, BytesMut};
use http::{Request, Response};
use tower::{Layer, Service};

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
    type Response = Response<Bytes>;
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
    type Output = Result<Response<Bytes>, E>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let response = if let Some(response) = this.response {
            response
        } else {
            *this.response = Some(ready!(this.inner.poll(cx)?));
            return Poll::Pending;
        };

        match ready!(Pin::new(response.body_mut()).poll_data(cx)) {
            Some(Ok(mut chunk)) => {
                this.buffer
                    .as_mut()
                    .map(|buf| buf.extend(chunk.copy_to_bytes(chunk.remaining())));

                Poll::Pending
            }
            Some(Err(err)) => {
                todo!()
            }
            None => Poll::Ready(Ok(this
                .response
                .take()
                .unwrap()
                .map(|_| this.buffer.take().expect("TODO").into()))),
        }
    }
}
