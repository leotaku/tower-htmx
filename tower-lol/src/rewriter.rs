use crate::either::EitherBody;
use crate::util::{ErrorBody, UnsafeSend};
use bytes::{Buf, Bytes, BytesMut};
use http::header::Entry;
use http::{Request, Response};
use http_body::{Body, Full};
use lol_html::errors::RewritingError;
use lol_html::{HtmlRewriter, Settings};
use std::error::Error;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};

pub trait SettingsProvider {
    fn set_request(&mut self, req: &http::request::Parts);
    fn provide<'b, 'a: 'b>(
        &mut self,
        res: &'a mut http::response::Parts,
    ) -> Option<Settings<'b, 'static>>;
}

#[derive(Debug, Clone)]
pub struct HtmlRewriterLayer<C> {
    settings: C,
}

impl<C> HtmlRewriterLayer<C> {
    pub fn new(settings: C) -> Self {
        Self { settings }
    }
}

impl<S, C: Clone> Layer<S> for HtmlRewriterLayer<C> {
    type Service = HtmlRewriterService<C, S>;

    fn layer(&self, inner: S) -> Self::Service {
        Self::Service::new(inner, self.settings.clone())
    }
}

#[derive(Clone, Debug)]
pub struct HtmlRewriterService<C, S> {
    settings: C,
    inner: S,
}

impl<C, S> HtmlRewriterService<C, S> {
    pub fn new(inner: S, settings: C) -> Self {
        Self { inner, settings }
    }
}

type TriBody<A, B, C> = EitherBody<A, EitherBody<B, C>>;

type HtmlRewriterBody<ResBody, ResBodyData> =
    TriBody<Full<Bytes>, ResBody, ErrorBody<ResBodyData, RewritingError>>;

impl<S, C, ReqBody, ResBody> Service<Request<ReqBody>> for HtmlRewriterService<C, S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone,
    C: SettingsProvider + Clone,
    ResBody: Body + Unpin,
    ResBody::Error: Error + Send + Sync + 'static,
{
    type Response = Response<HtmlRewriterBody<ResBody, ResBody::Data>>;
    type Error = S::Error;
    type Future = impl Future<Output = Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut cloned = self.clone();

        Box::pin(async move {
            std::future::poll_fn(|cx| cloned.poll_ready(cx)).await?;

            let req = {
                let (parts, body) = req.into_parts();
                cloned.settings.set_request(&parts);
                Request::from_parts(parts, body)
            };

            let res = cloned.inner.call(req).await?;

            let (mut parts, mut body) = res.into_parts();

            let settings = match provide_settings(&mut cloned.settings, &mut parts) {
                Some(settings) => settings,
                none @ None => {
                    drop(none);
                    return Ok(Response::from_parts(
                        parts,
                        EitherBody::b(EitherBody::a(body)),
                    ));
                }
            };

            let output = match handle_rewrite(settings, Pin::new(&mut body)).await {
                Ok(output) => output,
                Err(error) => {
                    return Ok(Response::from_parts(
                        parts,
                        EitherBody::b(EitherBody::b(ErrorBody::new(error))),
                    ))
                }
            };

            if let Entry::Occupied(mut entry) = parts.headers.entry(http::header::CONTENT_LENGTH) {
                entry.insert(output.len().into());
            }

            Ok(Response::from_parts(
                parts,
                EitherBody::a(Full::new(output.into())),
            ))
        })
    }
}


async fn handle_rewrite<'a, B>(
    settings: UnsafeSend<Settings<'a, 'static>>,
    mut body: Pin<&mut B>,
) -> Result<BytesMut, RewritingError>
where
    B: http_body::Body,
    B::Error: Error + Send + Sync + 'static,
{
    let mut output = BytesMut::new();
    let mut rewriter = unsafe {
        UnsafeSend::new(HtmlRewriter::new(settings.inner, |chunk: &[u8]| {
            output.extend_from_slice(chunk)
        }))
    };

    while let Some(chunk) = body
        .data()
        .await
        .transpose()
        .map_err(|err| RewritingError::ContentHandlerError(Box::new(err)))?
    {
        rewriter.inner.write(chunk.chunk())?
    }

    rewriter.inner.end()?;

    Ok(output)
}

fn provide_settings<'b, 'a: 'b, S: SettingsProvider>(
    settings: &mut S,
    res: &'a mut http::response::Parts,
) -> Option<UnsafeSend<Settings<'b, 'static>>> {
    settings
        .provide(res)
        .map(|it| unsafe { UnsafeSend::new(it) })
}
