use std::time::Duration;

use rant::{Node, NodeOptions, Qos};

fn main() -> Result<(), rant::Error> {
    let node = Node::new("{{name}}", NodeOptions::new())?;
    let count = node.publisher::<f64>("{{Name}}/Count", Qos::new())?;
    node.settle(None);
    for i in 0u64.. {
        count.send(&(i as f64))?;
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}
