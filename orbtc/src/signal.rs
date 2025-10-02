pub async fn ctrl_c() {
    use tokio::select;
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate()).unwrap();
    let mut sigint = signal(SignalKind::interrupt()).unwrap();
    select! {
        _ = sigterm.recv() => info!("Recieved SIGTERM"),
        _ = sigint.recv() => info!("Recieved SIGTERM"),
    };
}
