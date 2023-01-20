#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config: rxh::config::Config =
        toml::from_str(&tokio::fs::read_to_string("rxh.toml").await?)?;

    println!("{config:?}");

    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

    for server in config.servers {
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::task::spawn(async move {
            rxh::Server::init(server)?
                .shutdown_on(async move { shutdown_rx.recv().await })
                .run()
                .await
        });
    }

    tokio::signal::ctrl_c()
        .await
        .expect("Could not listen for CTRL-C");

    shutdown_tx.send(()).unwrap();

    Ok(())
}
