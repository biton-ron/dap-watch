use std::io::{self, BufRead, Write};
use std::net::TcpStream;

/// This client is only for testing purposes, it provides an easy way to connect to the proxy and send dap messages as inline JSON.
/// Content-Type header would be added to each JSON for proper parsing by the server.
fn main() {
    let mut stream = TcpStream::connect("127.0.0.1:2500").unwrap();
    println!("Connected! Type JSON bodies to send as DAP messages:");

    for line in io::stdin().lock().lines() {
        let body = line.unwrap();
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        stream.write_all(message.as_bytes()).unwrap();
        println!("Sent!");
    }
}
