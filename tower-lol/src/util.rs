use std::error::Error;
use std::fmt::Display;

pub struct UnsafeSend<T> {
    pub inner: T,
}

impl<T> UnsafeSend<T> {
    pub unsafe fn new(inner: T) -> Self {
        Self { inner }
    }
}

unsafe impl<T> Send for UnsafeSend<T> {}

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
