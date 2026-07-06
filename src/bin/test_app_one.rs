use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut counter = 0;

    loop {
        println!("counter: {}", counter);
        counter = counter + 1;
        sleep(Duration::from_secs(5)).await;
    }
}
