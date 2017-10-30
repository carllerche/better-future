# Futures Borrow

Future-aware cell that can move borrows into futures and closures.

A future-aware borrow allows a value to be borrowed such that the borrow can be
moved into closures passed to `Future` combinators.

## Usage

To use `futures-borrow`, first add this to your `Cargo.toml`:

```toml
[dependencies]
futures-borrow = { git = "https://github.com/carllerche/better-future" }
```

Next, add this to your crate:

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
