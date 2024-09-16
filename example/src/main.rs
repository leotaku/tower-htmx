use std::net::SocketAddr;

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::Router;
use tokio::net::TcpListener;
use tower::service_fn;
use tower_htmx::HtmxRewriteLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let serve_dir = ServeDir::new("assets").not_found_service(service_fn(|_| async {
        Ok((StatusCode::NOT_FOUND, Html("<pre>not found</pre>")).into_response())
    }));

    let app = Router::new()
        .nest_service("/", serve_dir)
        .layer(HtmxRewriteLayer::new().infallible());

    let addr: SocketAddr = ([0, 0, 0, 0], 8080).into();
    eprintln!("listening on: http://{}/", addr);

    console_subscriber::init();
    axum::serve(TcpListener::bind(addr).await?, app).await?;

    Ok(())
}
