use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use lightpost::{build_app, AppState, BlogRepository};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = env::var_os("LIGHTPOST_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));
    std::fs::create_dir_all(&data_dir)?;

    let database_path = data_dir.join("blog.sqlite");
    let repository = BlogRepository::open(&database_path)?;
    let admin_token =
        env::var("LIGHTPOST_ADMIN_TOKEN").unwrap_or_else(|_| "change-me-in-production".to_owned());
    let state = AppState::new(Arc::new(repository), admin_token);
    let app = build_app(state);

    let port = env::var("LIGHTPOST_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(address).await?;
    println!("LightPost listening on http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}
