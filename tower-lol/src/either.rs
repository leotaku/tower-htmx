use std::{error::Error, fmt::Display};

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

impl<A: Error, B: Error> Error for EitherError<A, B> {}

impl<A: Display, B: Display> Display for EitherError<A, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EitherError::A(a) => a.fmt(f),
            EitherError::B(b) => b.fmt(f),
        }
    }
}
