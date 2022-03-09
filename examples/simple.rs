// begin rustme snippet: example
use watchable::{Watchable, Watcher};

fn main() {
    // Create a Watchable<u32> which holds a u32 and notifies watchers when the
    // contained value changes.
    let watchable = Watchable::default();
    // Create a watcher that will efficiently be able to monitor and read the
    // contained value as it is updated.
    let watcher = watchable.watch();
    // Spawn a background worker that will print out the values the watcher reads.
    let watching_thread = std::thread::spawn(|| watching_thread(watcher));

    // Store a sequence of values. Each time a new value is written, any waiting
    // watchers will be notified there is a new value available.
    for i in 1_u32..=1000 {
        watchable.replace(i);
    }

    // Once we're done sending values, dropping the Watchable will ensure
    // watchers are notified of the disconnection. Watchers are guaranteed to be
    // able to read the final value.
    drop(watchable);

    // Wait for the thread to exit.
    watching_thread.join().unwrap();
}

fn watching_thread(watcher: Watcher<u32>) {
    // A Watcher can be used as an iterator which always reads the most
    // recent value, or parks the current thread until a new value is available.
    for value in watcher {
        // The value we read will not necessarily be sequential, even though the
        // main thread is storing a complete sequence.
        println!("Read value: {value}");
    }
}
// end rustme snippet: example

#[test]
fn runs() {
    main()
}
