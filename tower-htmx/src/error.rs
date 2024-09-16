use std::fmt::Display;
use std::future::Future;

use bytes::Bytes;
use http::{Request, Response};
use tower::Service;

use crate::HtmxRewriteService;

#[derive(Debug)]
pub enum Error<F, B, R> {
    Future(F),
    Body(B),
    Rewrite(R),
}

impl<F: Display, B: Display, R: Display> Error<F, B, R> {
    /// Format the given error as HTML.
    pub fn to_html(self) -> String {
        match self {
            Error::Future(err) => format!("future: {err}"),
            Error::Body(err) => format!("future: {err}"),
            Error::Rewrite(err) => format!("rewrite: {err}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HandleErrorService<S>(pub S);

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for HandleErrorService<HtmxRewriteService<S>>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>, Error = std::convert::Infallible>
        + Clone,
    ReqBody: Default,
    ResBody: http_body::Body<Data = Bytes>,
    ResBody::Error: Display,
{
    type Response = Response<http_body_util::Either<ResBody, http_body_util::Full<Bytes>>>;
    type Error = std::convert::Infallible;
    type Future = impl Future<Output = Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0 .0.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let fut = self.0.call(req);
        async {
            match fut.await {
                Ok(res) => Ok(res),
                Err(err) => {
                    let message = err.to_html();
                    Ok(Response::builder()
                        .status(http::status::StatusCode::INTERNAL_SERVER_ERROR)
                        .header(http::header::CONTENT_TYPE, "text/html")
                        .header(http::header::CONTENT_LENGTH, message.len())
                        .body(http_body_util::Either::Right(http_body_util::Full::new(
                            message.into(),
                        )))
                        .expect(""))
                }
            }
        }
    }
}
