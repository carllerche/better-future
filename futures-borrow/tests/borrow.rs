extern crate futures;
extern crate futures_borrow;
extern crate futures_test;

use futures_borrow::*;
use futures_test::Harness;

#[test]
fn test_basic_borrow() {
    let mut s = Borrow::new("hello".to_string());

    {
        // Ready immediately
        let mut ready = Harness::poll_fn(|| s.poll_ready());
        assert!(ready.poll().unwrap().is_ready());
    }

    // borrow
    let mut b = s.try_borrow().unwrap();

    {
        // Can't double borrow
        assert!(s.try_borrow().is_err());

        let mut ready = Harness::poll_fn(|| s.poll_ready());

        // Not ready
        assert!(!ready.poll().unwrap().is_ready());

        b.push_str("-world");

        drop(b);

        // Ready notified
        assert!(ready.is_notified());
    }

    {
        // Now ready
        let mut ready = Harness::poll_fn(|| s.poll_ready());
        assert!(ready.poll().unwrap().is_ready());
    }

    // Borrow again
    let b = s.try_borrow().unwrap();

    assert_eq!(*b, "hello-world");
}

#[test]
fn test_borrow_map() {
    let s = Borrow::new(vec!["hello".to_string()]);

    // Borrow
    let b = s.try_borrow().unwrap();
    let mut b = Borrow::map(b, |vec| &mut vec[0]);

    // Can't double borrow
    assert!(s.try_borrow().is_err());

    b.push_str("-world");
    drop(b);

    let b = s.try_borrow().unwrap();

    assert_eq!(b[0], "hello-world");
}
