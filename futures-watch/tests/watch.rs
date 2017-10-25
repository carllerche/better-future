extern crate futures;
extern crate futures_test;
extern crate futures_watch;

use futures::{Stream};
use futures::future::poll_fn;
use futures_test::Harness;
use futures_watch::*;

#[test]
fn smoke() {
    let (mut watch, mut store) = Watch::new("one");

    // Check the value
    assert_eq!(*watch.borrow(), "one");
    assert!(!watch.is_final());

    {
        let mut harness = Harness::new(poll_fn(|| watch.poll()));
        assert!(!harness.poll().unwrap().is_ready());

        // Change the value.
        assert_eq!(store.store("two").unwrap(), "one");

        // The watch was notified
        assert!(harness.poll().unwrap().is_ready());
    }

    assert!(!watch.is_final());
    assert_eq!(*watch.borrow(), "two");

    {
        let mut harness = Harness::new(poll_fn(|| watch.poll()));
        assert!(!harness.poll().unwrap().is_ready());

        // Dropping `store` notifies watches
        drop(store);

        // The watch was notified
        assert!(harness.poll().unwrap().is_ready());
    }

    assert!(watch.is_final());
    assert_eq!(*watch.borrow(), "two");
}

#[test]
fn multiple_watches() {
    let (mut watch1, mut store) = Watch::new("one");
    let mut watch2 = watch1.clone();

    {
        let mut h1 = Harness::new(poll_fn(|| watch1.poll()));
        let mut h2 = Harness::new(poll_fn(|| watch2.poll()));

        assert!(!h1.poll().unwrap().is_ready());

        // Change the value.
        assert_eq!(store.store("two").unwrap(), "one");

        // The watch was notified
        assert!(h1.poll().unwrap().is_ready());
        assert!(h2.poll().unwrap().is_ready());
    }

    assert_eq!(*watch1.borrow(), "two");
    assert_eq!(*watch2.borrow(), "two");
}
