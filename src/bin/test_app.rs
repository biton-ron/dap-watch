use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    loop {
        println!("lets change the text here...");
        sleep(Duration::from_secs(5)).await;
    }
}
