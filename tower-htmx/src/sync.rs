use bytes::{Bytes, BytesMut};
use http::Response;
use lol_html::{element, HtmlRewriter, Settings};

pub(crate) fn rewrite<Data: bytes::Buf>(
    input: &[u8],
    mut list: Vec<Response<http_body_util::Collected<Data>>>,
) -> Result<Bytes, lol_html::errors::RewritingError> {
    let mut result = BytesMut::new();

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!(
                r#"[hx-get][hx-trigger~="server"]"#, // break
                |el| {
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

                    let body = body.to_bytes();
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
                }
            )],
            ..Settings::default()
        },
        |chunk: &[u8]| result.extend_from_slice(chunk),
    );

    rewriter.write(&input).unwrap();
    rewriter.end().unwrap();

    Ok(result.into())
}

pub(crate) fn scan_tags(bytes: &[u8]) -> Result<Vec<String>, lol_html::errors::RewritingError> {
    let mut hrefs = Vec::new();
    let mut tag_scanner = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!(
                r#"[hx-get][hx-trigger~="server"]"#, // break
                |el| {
                    if let Some(href) = el.get_attribute("hx-get") {
                        hrefs.push(href)
                    }

                    Ok(())
                }
            )],
            ..Settings::default()
        },
        |_: &[u8]| (),
    );

    tag_scanner.write(bytes)?;
    drop(tag_scanner);

    Ok(hrefs)
}
