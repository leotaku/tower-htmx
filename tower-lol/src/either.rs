use bytes::Buf;
use std::error::Error;
use std::fmt::Display;

pin_project_lite::pin_project! {
    #[project = EitherBodyProj]
    pub enum EitherBody<A, B> {
        A{ #[pin] a: A },
        B{ #[pin] b: B },
    }
}

impl<A, B> EitherBody<A, B> {
    pub fn a(a: A) -> Self {
        Self::A { a }
    }

    pub fn b(b: B) -> Self {
        Self::B { b }
    }
}

impl<A, B, D, EA, EB> http_body::Body for EitherBody<A, B>
where
    A: http_body::Body<Data = D, Error = EA>,
    B: http_body::Body<Data = D, Error = EB>,
    D: Buf,
{
    type Data = D;
    type Error = EitherError<A::Error, B::Error>;

    fn poll_data(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.project() {
            EitherBodyProj::A { a } => a.poll_data(cx).map_err(EitherError::A),
            EitherBodyProj::B { b } => b.poll_data(cx).map_err(EitherError::B),
        }
    }

    fn poll_trailers(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        match self.project() {
            EitherBodyProj::A { a } => a.poll_trailers(cx).map_err(EitherError::A),
            EitherBodyProj::B { b } => b.poll_trailers(cx).map_err(EitherError::B),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EitherError<A, B> {
    A(A),
    B(B),
}

impl<A: Error, B: Error> Error for EitherError<A, B> {}

impl<A: Display, B: Display> Display for EitherError<A, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EitherError::A(a) => a.fmt(f),
            EitherError::B(b) => b.fmt(f),
        }
    }
}
