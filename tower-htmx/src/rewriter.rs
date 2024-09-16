use bytes::{Bytes, BytesMut};
use http::{Request, Response, Uri};
use lol_html::errors::RewritingError;
use lol_html::{element, HtmlRewriter, Settings};

pub(crate) enum Resolvable {
    Req(http::request::Builder),
    Rsp(http::response::Response<Bytes>),
}

pub(crate) trait Rewriter {
    type Error;

    fn match_request<ReqBody>(&mut self, req: Request<ReqBody>) -> (Request<ReqBody>, bool);
    fn match_response<ResBody>(&mut self, res: &Response<ResBody>) -> bool;
    fn extract_info(&mut self, data: &[u8]) -> Result<Vec<Resolvable>, Self::Error>;
    fn rewrite(&mut self, data: &[u8], list: Vec<Response<Bytes>>) -> Result<Bytes, Self::Error>;
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BasicRewriter {
    base_url: Uri,
    recursion_level: usize,
    selector: String,
    templated_child: Option<String>,
}

#[derive(Debug, Clone)]
struct RecursionProtector(usize);

impl BasicRewriter {
    pub(crate) fn new() -> Self {
        Self {
            base_url: Default::default(),
            recursion_level: 0,
            selector: r#"[hx-get][hx-trigger~="server"]"#.to_owned(),
            templated_child: None,
        }
    }
}

impl Rewriter for BasicRewriter {
    type Error = RewritingError;

    fn match_request<ReqBody>(
        &mut self,
        mut req: http::Request<ReqBody>,
    ) -> (http::Request<ReqBody>, bool) {
        self.base_url = req.uri().clone();
        self.recursion_level = match req.extensions().get() {
            Some(RecursionProtector(level)) => *level + 1,
            None => 0,
        };
        let req = match req
            .uri()
            .query()
            .and_then(|it| it.rsplit_once("parent=")?.1.split(&['#', '?']).next())
            .map(|it| it.to_owned())
        {
            Some(parent) => {
                let uri = req.uri_mut();
                self.templated_child = uri.path_and_query().map(|it| it.path().to_string());
                *uri = expand_uri(uri, parent).unwrap();

                req
            }
            None => req,
        };

        (req, self.recursion_level < 10)
    }

    fn match_response<ResBody>(&mut self, res: &http::Response<ResBody>) -> bool {
        res.headers().contains_key(http::header::CONTENT_LENGTH)
            && res
                .headers()
                .get(http::header::CONTENT_TYPE)
                .and_then(|it| it.to_str().ok())
                .is_some_and(|it| it.starts_with("text/html"))
    }

    fn extract_info(&mut self, data: &[u8]) -> Result<Vec<Resolvable>, Self::Error> {
        let mut reqs = Vec::new();
        let mut tag_scanner = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![element!(self.selector, |el| {
                    if let Some(href) = el.get_attribute("hx-get") {
                        let reference = if href == "[child]" {
                            self.templated_child.take()
                        } else {
                            Some(href)
                        };

                        reqs.push(match reference {
                            Some(href) => Resolvable::Req(
                                Request::builder()
                                    .method(http::Method::GET)
                                    .uri(expand_uri(&self.base_url, href)?)
                                    .extension(RecursionProtector(self.recursion_level)),
                            ),
                            None => Resolvable::Rsp(
                                Response::builder()
                                    .header(http::header::CONTENT_TYPE, "text/html")
                                    .body(Bytes::new())?,
                            ),
                        });
                    }

                    Ok(())
                })],
                ..Settings::default()
            },
            |_: &[u8]| (),
        );

        tag_scanner.write(data)?;
        drop(tag_scanner);

        Ok(reqs)
    }

    fn rewrite(
        &mut self,
        data: &[u8],
        mut list: Vec<Response<Bytes>>,
    ) -> Result<Bytes, Self::Error> {
        let mut result = BytesMut::new();

        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: vec![element!(self.selector, |el| {
                    let (parts, body) = list
                        .pop()
                        .expect("this to be called the right amount of times")
                        .into_parts();

                    let response_is_html = parts
                        .headers
                        .get(http::header::CONTENT_TYPE)
                        .map(|it| it.as_ref().starts_with(b"text/html"))
                        .unwrap_or(false);

                    let ct = response_is_html
                        .then_some(lol_html::html_content::ContentType::Html)
                        .unwrap_or(lol_html::html_content::ContentType::Text);

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
                })],
                ..Settings::default()
            },
            |chunk: &[u8]| result.extend_from_slice(chunk),
        );

        rewriter.write(&data)?;
        rewriter.end()?;

        Ok(result.into())
    }
}

fn expand_uri(uri: &http::uri::Uri, path: String) -> http::Result<http::uri::Uri> {
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = if path.starts_with("/") {
        Some(path.try_into()?)
    } else {
        Some(format!("{}/{}", uri.path(), path).try_into()?)
    };

    Ok(Uri::from_parts(parts)?)
}
