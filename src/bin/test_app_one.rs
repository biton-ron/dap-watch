use std::thread;
use std::time::Duration;

fn main() {
    let mut counter = 0;

    loop {
        println!("counter: {}", counter);
        counter += 1;
        thread::sleep(Duration::from_secs(5));
    }
}
