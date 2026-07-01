use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let x = 5;
    println!("{} = 5", x);

    loop {
        println!("This is test app one with a file edit test155");
        sleep(Duration::from_secs(5)).await;
    }
}
