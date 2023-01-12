#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = serde_json::from_str(&tokio::fs::read_to_string("rxh.json").await?)?;
    let (_address, server) = rxh::server::init(config, tokio::signal::ctrl_c())?;
    server.await?;
    Ok(())
}
