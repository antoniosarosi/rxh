#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = serde_json::from_str(&tokio::fs::read_to_string("rxh.json").await?)?;
    rxh::server::start(config, tokio::signal::ctrl_c()).await
}
