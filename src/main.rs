#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = toml::from_str(&tokio::fs::read_to_string("rxh.toml").await?)?;

    rxh::Master::init(config)?
        .shutdown_on(tokio::signal::ctrl_c())
        .run()
        .await?;

    Ok(())
}
