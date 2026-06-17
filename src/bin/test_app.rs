use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    loop {
        println!("This one maybe");
        // Test
        // Two lines down
        println!("lets change something in here...");

        //
        //

        println!("maybe this one?");
        sleep(Duration::from_secs(5)).await;
    }
}
