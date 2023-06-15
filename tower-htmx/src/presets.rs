use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Not;

use tower_lol::lol_html::html_content::{Element, UserData};
use tower_lol::lol_html::{self, Settings};
use tower_lol::resolve::ResolveContext;
use tower_lol::rewrite::SettingsProvider;

#[derive(Debug, Clone)]
pub struct ExtractSettings {}

impl ExtractSettings {
    pub fn new() -> Self {
        Self {}
    }
}

impl SettingsProvider for ExtractSettings {
    fn handle_request(&mut self, _req: &http::request::Parts) {}

    fn handle_response<'b, 'a: 'b>(
        &mut self,
        res: &'a mut http::response::Parts,
    ) -> Option<Settings<'b, 'static>> {
        res.extensions.insert(ResolveContext::new());
        let map = res.extensions.get_mut::<ResolveContext>().unwrap();

        Some(Settings {
            element_content_handlers: vec![lol_html::element!("[hx-get]", move |el| {
                let path = get_query_string(el, "hx-get");
                map.entries.insert(path, None);

                Ok(())
            })],
            ..Settings::default()
        })
    }
}

#[derive(Debug, Clone)]
pub struct InsertSettings {}

impl InsertSettings {
    pub fn new() -> Self {
        Self {}
    }
}

impl SettingsProvider for InsertSettings {
    fn handle_request(&mut self, _req: &http::request::Parts) {}

    fn handle_response<'b, 'a: 'b>(
        &mut self,
        res: &'a mut http::response::Parts,
    ) -> Option<Settings<'b, 'static>> {
        let map = res.extensions.remove::<ResolveContext>().unwrap();

        Some(Settings {
            element_content_handlers: vec![lol_html::element!("[hx-get]", move |el| {
                let attr = get_query_string(el, "hx-get");
                let content = std::str::from_utf8(
                    map.entries
                        .get(&attr)
                        .and_then(|it| it.as_ref())
                        .ok_or("problem with inner content")?
                        .body(),
                )?;

                let ct = lol_html::html_content::ContentType::Html;

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
        })
    }
}

#[derive(Debug, Clone)]
pub struct SelectSettings {
    selector: Option<String>,
}

impl SelectSettings {
    pub fn new() -> Self {
        Self { selector: None }
    }
}

impl SettingsProvider for SelectSettings {
    fn handle_request(&mut self, req: &http::request::Parts) {
        fn inner(req: &http::request::Parts) -> Option<String> {
            let url = form_urlencoded::parse(req.uri.query()?.as_bytes());
            let query: HashMap<Cow<'_, str>, Cow<'_, str>> = url.collect();
            let selector = query.get("hx-select")?;

            selector
                .is_empty()
                .not()
                .then(|| format!("{selector}, {selector} *",))
        }

        self.selector = inner(req);
    }

    fn handle_response<'b, 'a: 'b>(
        &mut self,
        _res: &'a mut http::response::Parts,
    ) -> Option<Settings<'b, 'static>> {
        let selector = self.selector.take()?;

        Some(Settings {
            element_content_handlers: vec![
                lol_html::element!(selector, |el| {
                    el.set_user_data(true);

                    Ok(())
                }),
                lol_html::text!(selector, |el| {
                    el.set_user_data(true);

                    Ok(())
                }),
                lol_html::element!("*", |el| {
                    let user_data = el.user_data().downcast_ref::<bool>();
                    if !user_data.copied().unwrap_or(false) {
                        el.remove_and_keep_content();
                    }

                    Ok(())
                }),
            ],
            document_content_handlers: vec![
                lol_html::doctype!(|el| {
                    el.remove();

                    Ok(())
                }),
                lol_html::doc_text!(|el| {
                    let user_data = el.user_data().downcast_ref::<bool>();
                    if !user_data.copied().unwrap_or(false) {
                        el.remove();
                    }

                    Ok(())
                }),
                lol_html::doc_comments!(|el| {
                    el.remove();

                    Ok(())
                }),
            ],
            ..Settings::default()
        })
    }
}

fn get_query_string(el: &Element, attr: &str) -> String {
    let mut path = el.get_attribute(attr).expect("attr was required");

    if let Some(query) = el.get_attribute("hx-select") {
        path.push_str("?hx-select=");
        path.extend(percent_encoding::utf8_percent_encode(
            &query,
            percent_encoding::NON_ALPHANUMERIC,
        ))
    }

    path
}
