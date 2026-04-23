#![warn(clippy::pedantic, clippy::nursery)]

#[tokio::main]
async fn main() {
    cortex_app::run().await;
}
