# Better Future

We all wish for a better future -- self-driving cars, fusion power, and UBI.
This repository does not provide any of this. Instead, it has a collection of
utilities that may make working with [futures-rs](http://github.com/alexcrichton/futures-rs)
easier.

[![Travis Build Status](https://travis-ci.org/carllerche/better-future.svg?branch=master)](https://travis-ci.org/carllerche/better-future)

### `futures-borrow`

Future-aware cell that can move borrows into futures and closures.

[Documentation](https://carllerche.github.io/better-future/futures_borrow/)

```rust
extern crate futures;
extern crate futures_borrow;

use futures::*;
use futures_borrow::Borrow;

fn main() {
    let borrow = Borrow::new("hello".to_string());

    // Acquire a borrow
    let b = borrow.try_borrow().unwrap();

    // The borrow is in use
    assert!(!borrow.is_ready());

    // Use the borrow in a closure
    future::ok::<_, ()>(()).and_then(move |_| {
        println!("value={}", &*b);
        Ok(())
    }).wait().unwrap();

    // A new borrow can be made
    assert!(borrow.is_ready());
}
```

### `futures-test`

Utilities for testing futures based code.

[Documentation](https://carllerche.github.io/better-future/futures_test/)

```rust
extern crate futures;
extern crate futures_test;

use futures::*;
use futures::sync::mpsc;
use futures_test::Harness;

fn main() {
    let (tx, rx) = mpsc::channel(10);
    let mut rx = Harness::new(rx);

    assert!(!rx.is_notified());
    // The future is polled out of a task context.
    assert!(!rx.poll_next().unwrap().is_ready());

    tx.send("hello").wait().unwrap();

    assert!(rx.is_notified());
}
```

### `futures-watch`

A multi-consumer, single producer cell that receives notifications when the
inner value is changed. This allows for efficiently broadcasting values to
multiple watchers. This can be useful for situations like updating configuration
values throughout a system.

[Documentation](https://carllerche.github.io/better-future/futures_watch/)

```rust
extern crate futures;
extern crate futures_watch;

use futures::{Future, Stream};
use futures_watch::Watch;
use std::thread;

fn main() {
    let (watch, mut store) = Watch::new("hello");

    thread::spawn(move || {
        store.store("goodbye");
    });

    watch.into_future()
        .and_then(|(_, watch)| {
            assert_eq!(*watch.borrow(), "goodbye");
            Ok(())
        })
        .wait().unwrap();
}
```

## License

The provided software is licensed under the terms of both the MIT license and
the Apache License (Version 2.0), with portions covered by various BSD-like
licenses.

See LICENSE-APACHE, and LICENSE-MIT for details.
