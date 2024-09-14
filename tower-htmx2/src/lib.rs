#![feature(impl_trait_in_assoc_type)]
#![forbid(unused_unsafe)]
//#![warn(clippy::all, missing_docs, nonstandard_style, future_incompatible)]
#![allow(clippy::type_complexity)]

use std::future::{poll_fn, Future};
use std::mem;
use std::pin::pin;

use bytes::{Buf, Bytes, BytesMut};
use http::{Request, Response, Uri};
use http_body_util::BodyExt;
use lol_html::{element, HtmlRewriter, Settings};
use tower::{Layer, Service};

#[derive(Debug, Clone)]
pub struct HtmxRewriteLayer;

impl HtmxRewriteLayer {
    pub fn new() -> Self {
        HtmxRewriteLayer
    }
}

impl<S> Layer<S> for HtmxRewriteLayer {
    type Service = HtmxRewriteService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HtmxRewriteService::new(inner)
    }
}

#[derive(Debug, Clone)]
pub struct HtmxRewriteService<S>(S);

impl<S> HtmxRewriteService<S> {
    fn new(inner: S) -> Self {
        HtmxRewriteService(inner)
    }
}

impl<S, ReqBody, ResBody> Service<http::Request<ReqBody>> for HtmxRewriteService<S>
where
    S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>> + Clone,
    S::Error: std::error::Error + Send + Sync + 'static,
    ReqBody: Default,
    ResBody: http_body::Body,
{
    type Response = Response<http_body_util::Full<Bytes>>;
    type Error = S::Error;
    type Future = impl Future<Output = Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        // if req.extensions().get::<RecursionProtector>().is_some() {
        //     panic!("recursion detected")
        // };

        let uri = req.uri().clone();
        dbg!(&uri);
        let mut cloned = mem::replace(self, self.clone());
        let fut = self.0.call(req);

        async move {
            let cloned = &mut cloned;
            let handle = tokio::runtime::Handle::current();
            let res = fut.await?;

            let (parts, body) = res.into_parts();
            let original = body.collect().await.ok().unwrap().to_bytes();
            let mut new = BytesMut::new();

            let mut rewriter = HtmlRewriter::new(
                Settings {
                    element_content_handlers: vec![
                        // Rewrite insecure hyperlinks
                        element!(r#"[hx-get][hx-trigger~="server"]"#, |el| {
                            let attr = el.get_attribute("hx-get").unwrap();
                            let req = Request::builder()
                                .method(http::method::Method::GET)
                                .uri(expand_uri(&uri, attr)?)
                                .extension(RecursionProtector)
                                .body(Default::default())?;

                            let rsp = tokio::task::block_in_place(|| {
                                handle.block_on(async {
                                    poll_fn(|cx| cloned.poll_ready(cx)).await?;
                                    let (parts, body) = cloned.call(req).await?.into_parts();
                                    Ok::<_, Self::Error>(Response::from_parts(
                                        parts,
                                        body.collect().await,
                                    ))
                                })
                            })?;

                            let response_is_html = rsp
                                .headers()
                                .get(http::header::CONTENT_TYPE)
                                .map(|it| it.as_ref().starts_with(b"text/html"))
                                .unwrap_or(false);

                            let ct = response_is_html
                                .then_some(lol_html::html_content::ContentType::Html)
                                .unwrap_or(lol_html::html_content::ContentType::Text);

                            let body = rsp.into_body()?.to_bytes();
                            let content = std::str::from_utf8(&body)?;

                            match el.get_attribute("hx-swap").as_deref() {
                                None | Some("innerHTML") => el.set_inner_content(content, ct),
                                Some("outerHTML") => el.replace(content, ct),
                                Some("afterbegin") => el.prepend(content, ct),
                                Some("beforebegin") => el.before(content, ct),
                                Some("beforeend") => el.append(content, ct),
                                Some("afterend") => el.after(content, ct),
                                Some("delete") => el.remove(),
                                _ => (),
                            }

                            Ok(())
                        }),
                    ],
                    ..Settings::default()
                },
                |chunk: &[u8]| new.extend_from_slice(chunk),
            );

            rewriter.write(&original).unwrap();

            dbg!(&new);

            Ok(http::Response::from_parts(
                parts,
                http_body_util::Full::new(new.into()),
            ))
        }
    }
}

#[derive(Debug, Clone)]
struct RecursionProtector;

fn expand_uri(uri: &http::uri::Uri, path: String) -> http::Result<http::uri::Uri> {
    let mut p_and_q = if path.starts_with("/") {
        path
    } else {
        format!("{}/{}", uri.path(), path)
    };

    if let Some(query) = uri.query() {
        p_and_q.push_str(query);
    }

    Uri::builder()
        .scheme(uri.scheme().cloned().unwrap_or(http::uri::Scheme::HTTP))
        .authority(
            uri.authority()
                .cloned()
                .unwrap_or("internal".try_into().unwrap())
        )
        .path_and_query(p_and_q)
        .build()
}

#[derive(Debug, Clone)]
pub struct DummyLayer;

impl DummyLayer {
    pub fn new() -> Self {
        DummyLayer
    }
}

impl<S> Layer<S> for DummyLayer {
    type Service = DummyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DummyService::new(inner)
    }
}

#[derive(Debug, Clone)]
pub struct DummyService<S>(S);

impl<S> DummyService<S> {
    fn new(inner: S) -> Self {
        DummyService(inner)
    }
}

impl<S, ReqBody> Service<http::Request<ReqBody>> for DummyService<S>
where
    S: Service<http::Request<ReqBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        todo!()
    }
}
