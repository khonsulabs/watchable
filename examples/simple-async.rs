// begin rustme snippet: example
use futures_util::StreamExt;
use watchable::{Watchable, Watcher};

#[tokio::main]
async fn main() {
    // Create the watchable container for our u32s.
    let watchable = Watchable::new(0);
    // Create a subscriber that watches for changes to the stored value.
    let watcher = watchable.subscribe();
    // Spawn a background worker that will print out the values it reads.
    let watching_task = tokio::task::spawn(watching_task(watcher));

    // Send a sequence of numbers, ending at 1,000.
    for i in 1..=1000 {
        watchable.replace(i);
    }

    // Wait for the thread to exit.
    watching_task.await.unwrap();
}

async fn watching_task(watcher: Watcher<u32>) {
    // A Watcher can be converted into a Stream, which allows for asynchronous
    // iteration.
    let mut stream = watcher.into_stream();
    while let Some(value) = stream.next().await {
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
