use axum::http::Request;
use axum::Router;
use tower_htmx::{SelectLayer, TemplateLayer};
use tower_http::services::ServeDir;
use tower_livereload::LiveReloadLayer;
use tracing_subscriber::util::SubscriberInitExt;

fn not_htmx_predicate<T>(req: &Request<T>) -> bool {
    !req.headers().contains_key("hx-request")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .nest_service("/", ServeDir::new("assets"))
        .layer(SelectLayer::new())
        .layer(TemplateLayer::new())
        .layer(LiveReloadLayer::new().request_predicate(not_htmx_predicate));

    let addr = ([0, 0, 0, 0], 8080).into();
    eprintln!("listening on: http://{}/", addr);

    tracing_subscriber::fmt().finish().init();
    axum::Server::try_bind(&addr)?
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
