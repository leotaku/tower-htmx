pin_project_lite::pin_project! {
    #[project = FutureProj]
    pub enum Either<L, R> {
        Left { #[pin] left: L },
        Right { #[pin] right: R },
    }
}

impl<L, R> Either<L, R> {
    pub fn left(left: L) -> Self {
        Self::Left { left }
    }

    pub fn right(right: R) -> Self {
        Self::Right { right }
    }
}

impl<L, R, Output> std::future::Future for Either<L, R>
where
    L: std::future::Future<Output = Output>,
    R: std::future::Future<Output = Output>,
{
    type Output = Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.project() {
            FutureProj::Left { left } => left.poll(cx),
            FutureProj::Right { right } => right.poll(cx),
        }
    }
}
