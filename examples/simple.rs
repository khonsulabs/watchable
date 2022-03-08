// begin rustme snippet: example
use watchable::{Watchable, Watcher};

fn main() {
    // Create the watchable container for our u32s.
    let watchable = Watchable::new(0);
    // Create a subscriber that watches for changes to the stored value.
    let watcher = watchable.subscribe();
    // Spawn a background worker that will print out the values it reads.
    let watching_thread = std::thread::spawn(|| watching_thread(watcher));

    // Send a sequence of numbers, ending at 1,000.
    for i in 1..=1000 {
        watchable.replace(i);
    }

    // Wait for the thread to exit.
    watching_thread.join().unwrap();
}

fn watching_thread(watcher: Watcher<u32>) {
    // A Watcher can be used as an iterator which always produces the most
    // recent value, or parks the current thread until a new value is available.
    for value in watcher {
        // The value we received will not necessarily be sequential, even though
        // the main thread is publishing a complete sequence.
        println!("Read value: {value}");
        if value == 1000 {
            break;
        }
    }
}
// end rustme snippet: example

#[test]
fn runs() {
    main()
}
