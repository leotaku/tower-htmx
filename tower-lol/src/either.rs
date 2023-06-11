use std::{
    error::Error,
    fmt::Display,
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};

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

pin_project_lite::pin_project! {
    pub struct StoringFuture<O, F> {
        #[pin] future: F,
        output: Option<O>,
    }
}

impl<O, F> Future for StoringFuture<O, F>
where
    F: Future<Output = O>,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        if this.output.is_none() {
            let output = ready!(this.future.poll(cx));
            *this.output = Some(output);
        };

        Poll::Ready(())
    }
}

impl<O, F> StoringFuture<O, F> {
    pub fn new(future: F) -> Self {
        Self {
            future,
            output: None,
        }
    }

    pub fn take(self: Pin<&mut Self>) -> O {
        self.project().output.take().unwrap()
    }
}
