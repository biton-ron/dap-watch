use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    loop {
        println!("This is test app one with a file edit");
        sleep(Duration::from_secs(5)).await;
    }
}
