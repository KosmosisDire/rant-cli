// Runs for a minute at most, so a failed test never leaves it running for long.
fn main() {
    println!("ready motor");
    std::thread::sleep(std::time::Duration::from_secs(60));
}
