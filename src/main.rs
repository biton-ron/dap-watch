use std::error::Error;

use crate::ide_server::IdeServer;

mod dap_stream;
mod ide_server;

#[tokio::main(flavor = "current_thread")]

async fn main() -> Result<(), Box<dyn Error>> {
    let mut ide = IdeServer::new(2500).await?;
    let mut ide_stream = ide.connect().await?;

    loop {
        let message = ide_stream.read().await?;

        println!("Your message is:");
        println!("{:?}", message);
    }
}
