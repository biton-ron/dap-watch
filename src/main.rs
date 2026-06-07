use std::error::Error;

use crate::ide_server::IdeServer;

mod ide_server;

#[tokio::main(flavor = "current_thread")]

async fn main() -> Result<(), Box<dyn Error>> {
    let mut ide = IdeServer::new(2500).await?;

    ide.connect().await?;

    loop {
        let message = ide.read().await?;

        println!("Your message is:");
        println!("{}", String::from_utf8_lossy(&message));
    }
}
