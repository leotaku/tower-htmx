use bytes::Buf;
use std::marker::PhantomData;

pin_project_lite::pin_project! {
    pub struct ErrorBody<D, E> {
        error: Option<E>,
        _phantom: PhantomData<D>,
    }
}

impl<D, E> ErrorBody<D, E> {
    pub fn new(error: E) -> Self {
        Self {
            error: Some(error),
            _phantom: PhantomData::default(),
        }
    }
}

impl<D: Buf, E> http_body::Body for ErrorBody<D, E> {
    type Data = D;
    type Error = E;

    fn poll_data(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Self::Data, Self::Error>>> {
        std::task::Poll::Ready(Some(Err(self
            .project()
            .error
            .take()
            .expect("no repeated calls after error"))))
    }

    fn poll_trailers(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        unreachable!()
    }
}

pub struct UnsafeSend<T> {
    pub inner: T,
}

impl<T> UnsafeSend<T> {
    pub unsafe fn new(inner: T) -> Self {
        Self { inner }
    }
}

unsafe impl<T> Send for UnsafeSend<T> {}

pin_project_lite::pin_project! {
    pub struct StoringFuture<O, F> {
        #[pin] future: F,
        output: Option<O>,
    }
}
