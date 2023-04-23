#[tokio::main]
async fn main() -> Result<(), rxh::Error> {
    let config: rxh::config::Config =
        toml::from_str(&tokio::fs::read_to_string("rxh.toml").await?)?;

    rxh::Master::init(config.into_normalized())?
        .shutdown_on(tokio::signal::ctrl_c())
        .run()
        .await
}
