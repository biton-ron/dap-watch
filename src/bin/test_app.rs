use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    loop {
        println!("test_app is iterarting...");
        sleep(Duration::from_secs(2)).await;
    }
}
