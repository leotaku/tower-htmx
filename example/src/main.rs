use std::net::SocketAddr;

use axum::error_handling::HandleErrorLayer;
use axum::http::{Request, StatusCode};
use axum::response::Html;
use axum::Router;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_htmx::HtmxRewriteLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new().nest_service("/", ServeDir::new("assets"));
    let app = app.layer(
        ServiceBuilder::new()
            .layer(HandleErrorLayer::new(
                |err: tower_htmx::Error<_, _>| async {
                    (StatusCode::INTERNAL_SERVER_ERROR, Html(err.to_html()))
                },
            ))
            .layer(HtmxRewriteLayer::new()),
    );

    let addr: SocketAddr = ([0, 0, 0, 0], 8080).into();
    eprintln!("listening on: http://{}/", addr);

    console_subscriber::init();
    axum::serve(TcpListener::bind(addr).await?, app).await?;

    Ok(())
}
