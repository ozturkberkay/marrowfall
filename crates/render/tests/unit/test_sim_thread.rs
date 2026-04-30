use std::thread;
use std::time::{Duration, Instant};

use game::Sim;
use render::sim_thread::spawn;

#[test]
fn sim_thread_ticks_and_shuts_down_cleanly() {
    let mut handle = spawn(Sim::new(1));

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let tick = handle.latest().tick;
        if tick > 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "sim thread produced no ticks within 2s"
        );
        thread::sleep(Duration::from_millis(5));
    }

    handle.shutdown();
}
