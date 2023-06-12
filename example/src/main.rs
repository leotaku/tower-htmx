#![feature(closure_lifetime_binder)]

use axum::Router;
use tower_htmx::{SubsetLayer, TemplateLayer};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().finish();

    let app = Router::new()
        .nest_service("/", ServeDir::new("."))
        .layer(SubsetLayer::new())
        .layer(TemplateLayer::new());

    let addr = ([0, 0, 0, 0], 8080).into();
    eprintln!("listening on: http://{}/", addr);

    tracing_subscriber::fmt::init();
    axum::Server::try_bind(&addr)?
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

// |req: &Request<Full<Bytes>>| {
//     let query = req.uri().query().map(|it| it.to_string());

//     async move {
//         let query = query?;
//         let url = url::form_urlencoded::parse(query.as_bytes());
//         let query: HashMap<Cow<'_, str>, Cow<'_, str>> = url.collect();
//         let selector = format!("{0}, {0} *", query.get("lol-path")?);

//         Some(Settings {
//             element_content_handlers: vec![
//                 lol_html::element!(selector, |el| {
//                     el.set_user_data(true);

//                     Ok(())
//                 }),
//                 lol_html::text!(selector, |el| {
//                     el.set_user_data(true);

//                     Ok(())
//                 }),
//                 lol_html::element!("*", |el| {
//                     let user_data = el.user_data().downcast_ref::<bool>();
//                     if !user_data.copied().unwrap_or(false) {
//                         el.remove_and_keep_content();
//                     }

//                     Ok(())
//                 }),
//             ],
//             document_content_handlers: vec![
//                 lol_html::doctype!(|el| {
//                     el.remove();

//                     Ok(())
//                 }),
//                 lol_html::doc_text!(|el| {
//                     let user_data = el.user_data().downcast_ref::<bool>();
//                     if !user_data.copied().unwrap_or(false) {
//                         el.remove();
//                     }

//                     Ok(())
//                 }),
//                 lol_html::doc_comments!(|el| {
//                     el.remove();

//                     Ok(())
//                 }),
//             ],
//             ..Settings::default()
//         })
//     }
// }
