mod ide_server;

use tokio::net::TcpListener;

#[tokio::main(flavor = "current_thread")]

async fn main() -> Result<(), Box<dyn Error>> {
    let port: u16 = 2005; // TODO: Should be taken from CLI/config file
    let proxy_server = TcpListener::bind(("127.0.0.1", port)).await?;
}
