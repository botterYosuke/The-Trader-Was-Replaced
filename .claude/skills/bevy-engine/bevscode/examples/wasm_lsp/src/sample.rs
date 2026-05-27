// Try editing this file in your browser. Hover identifiers, type, and the
// rust-analyzer instance running on the host (proxied through the WebSocket
// bridge) will respond with diagnostics, completions, and hover info.

fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

fn main() {
    let message = greet("bevscode");
    println!("{message}");

    let numbers: Vec<i32> = (1..=10).collect();
    let total: i32 = numbers.iter().sum();
    println!("sum of 1..=10 = {total}");
}
