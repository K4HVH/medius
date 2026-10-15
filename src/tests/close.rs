//! Closing a stream from another thread: the receiver blocked on it wakes at once.

#![cfg(feature = "mock")]

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::FrameType;
use crate::types::{Button, CatchFilter, Class, Direction, LogLevel, Usage};
use crate::{Device, Error, MockBox};

// Long enough that a receiver still blocked when it passes was never woken.
const WAKE: Duration = Duration::from_secs(2);

fn catch_frames(mock: &MockBox) -> Vec<u8> {
    mock.recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Catch)
        .map(|f| f.payload[4])
        .collect()
}

#[test]
fn closing_an_event_stream_wakes_a_receiver_blocked_on_a_clone() {
    let dev = Device::with_mock(MockBox::new());
    let s = dev.catch_events([CatchFilter::everything()]).unwrap();
    let blocked = s.clone();
    let (tx, rx) = mpsc::channel();
    let t = thread::spawn(move || tx.send(blocked.recv()).unwrap());
    thread::sleep(Duration::from_millis(50));
    let at = Instant::now();
    s.close();
    let got = rx.recv_timeout(WAKE).expect("the blocked recv woke");
    assert!(matches!(got, Err(Error::Disconnected)));
    assert!(at.elapsed() < Duration::from_millis(500));
    assert!(!s.is_connected());
    t.join().unwrap();
}

#[test]
fn a_closed_event_stream_unsubscribes_once() {
    let mock = MockBox::new();
    let dev = Device::with_mock(mock.clone());
    let s = dev.catch_events([CatchFilter::everything()]).unwrap();
    let other = s.clone();
    s.close();
    s.close();
    assert_eq!(catch_frames(&mock), vec![1, 0]);
    // The clones going later sends nothing more: the subscription already ended.
    drop(s);
    drop(other);
    assert_eq!(catch_frames(&mock), vec![1, 0]);
}

#[test]
fn a_closed_event_stream_returns_at_once_from_every_receive() {
    let dev = Device::with_mock(MockBox::new());
    let s = dev.catch_events([CatchFilter::everything()]).unwrap();
    s.close();
    assert!(matches!(s.recv(), Err(Error::Disconnected)));
    assert!(s.try_recv().is_none());
    let at = Instant::now();
    assert!(s.recv_timeout(WAKE).is_none());
    assert!(at.elapsed() < Duration::from_millis(500));
}

#[test]
fn a_closer_ends_an_input_stream_another_thread_is_receiving_on() {
    let dev = Device::with_mock(MockBox::new());
    let input = dev.input_events(CatchFilter::all_input()).unwrap();
    let closer = input.closer();
    let (tx, rx) = mpsc::channel();
    let t = thread::spawn(move || tx.send(input.recv()).unwrap());
    thread::sleep(Duration::from_millis(50));
    closer.close();
    let got = rx.recv_timeout(WAKE).expect("the blocked recv woke");
    assert!(matches!(got, Err(Error::Disconnected)));
    t.join().unwrap();
}

#[test]
fn an_input_stream_closes_like_its_event_stream() {
    let mock = MockBox::new();
    let dev = Device::with_mock(mock.clone());
    let input = dev.input_events(CatchFilter::all_input()).unwrap();
    input.close();
    assert!(!input.is_connected());
    assert!(matches!(input.recv(), Err(Error::Disconnected)));
    assert_eq!(catch_frames(&mock).last(), Some(&0));
}

#[test]
fn a_closed_input_stream_drops_what_it_decoded_and_has_not_returned() {
    let mock = MockBox::new();
    let dev = Device::with_mock(mock.clone());
    let input = dev
        .input_events([CatchFilter::watch_class(Class::Button)])
        .unwrap();
    // One snapshot, two presses: the first returns, the second waits in the decoded queue.
    mock.push_usages(
        0,
        1_000,
        Class::Button,
        Direction::PRESS,
        &[Usage::from(Button::LEFT), Usage::from(Button::RIGHT)],
    );
    assert!(input.recv_timeout(WAKE).is_some());
    input.close();
    assert!(matches!(input.recv(), Err(Error::Disconnected)));
    assert!(input.try_recv().is_none());
    assert!(input.recv_timeout(Duration::ZERO).is_none());
}

#[test]
fn closing_a_log_stream_wakes_its_receiver_and_leaves_the_others() {
    let mock = MockBox::new();
    let dev = Device::with_mock(mock.clone());
    let closing = dev.logs();
    let blocked = closing.clone();
    let other = dev.logs();
    let (tx, rx) = mpsc::channel();
    let t = thread::spawn(move || tx.send(blocked.recv()).unwrap());
    thread::sleep(Duration::from_millis(50));
    closing.close();
    let got = rx.recv_timeout(WAKE).expect("the blocked recv woke");
    assert!(matches!(got, Err(Error::Disconnected)));
    assert!(!closing.is_connected());
    assert!(closing.try_recv().is_none());
    t.join().unwrap();

    assert!(other.is_connected());
    mock.push_log(LogLevel::Info, "still here");
    let line = other
        .recv_timeout(WAKE)
        .expect("an open log stream still gets lines");
    assert_eq!(line.text, "still here");
}

#[test]
fn a_closer_outlives_the_stream_it_closes() {
    let dev = Device::with_mock(MockBox::new());
    let closer = dev.logs().closer();
    closer.close();
    closer.close();
    let closer = dev
        .catch_events([CatchFilter::everything()])
        .unwrap()
        .closer();
    closer.close();
}

#[test]
fn an_input_stream_recv_timeout_of_zero_reads_what_is_queued() {
    let mock = MockBox::new();
    let dev = Device::with_mock(mock.clone());
    let input = dev
        .input_events([CatchFilter::watch(Button::LEFT)])
        .unwrap();
    mock.push_usages(
        0,
        1_000,
        Class::Button,
        Direction::PRESS,
        &[Usage::from(Button::LEFT)],
    );
    // Asked again until the press has crossed from the mock: a recv_timeout of zero has to read what
    // is queued, not give up before looking.
    let start = Instant::now();
    while start.elapsed() < WAKE {
        if input.recv_timeout(Duration::ZERO).is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("recv_timeout(ZERO) never returned the queued press");
}

#[cfg(feature = "async")]
#[test]
fn closing_wakes_an_async_receiver_of_each_stream() {
    use futures::executor::block_on;
    let dev = Device::with_mock(MockBox::new());
    let events = dev.catch_events([CatchFilter::everything()]).unwrap();
    let logs = dev.logs();
    let (on_events, on_logs) = (events.clone(), logs.clone());
    let (tx, rx) = mpsc::channel();
    let tx2 = tx.clone();
    let a = thread::spawn(move || {
        tx.send(("events", block_on(on_events.recv_async()).is_err()))
            .unwrap()
    });
    let b = thread::spawn(move || {
        tx2.send(("logs", block_on(on_logs.recv_async()).is_err()))
            .unwrap()
    });
    thread::sleep(Duration::from_millis(50));
    events.close();
    logs.close();
    let mut got = vec![
        rx.recv_timeout(WAKE).unwrap(),
        rx.recv_timeout(WAKE).unwrap(),
    ];
    got.sort();
    assert_eq!(got, vec![("events", true), ("logs", true)]);
    a.join().unwrap();
    b.join().unwrap();
}

#[cfg(feature = "async")]
#[test]
fn a_closed_event_stream_ends_its_async_stream() {
    use futures::executor::block_on;
    let dev = Device::with_mock(MockBox::new());
    let events = dev.catch_events([CatchFilter::everything()]).unwrap();
    events.close();
    let mut s = std::pin::pin!(events.stream());
    let next = std::future::poll_fn(|cx| futures_core::Stream::poll_next(s.as_mut(), cx));
    assert!(block_on(next).is_none());
}

#[test]
fn an_input_stream_answers_its_accessors_while_another_thread_receives() {
    let dev = Device::with_mock(MockBox::new());
    let input = std::sync::Arc::new(dev.input_events(CatchFilter::all_input()).unwrap());
    let receiving = std::sync::Arc::clone(&input);
    let (tx, rx) = mpsc::channel();
    let t = thread::spawn(move || tx.send(receiving.recv()).unwrap());
    thread::sleep(Duration::from_millis(50));
    let at = Instant::now();
    assert!(input.held(Class::Button).is_empty());
    assert_eq!(input.dropped(), 0);
    assert!(input.is_connected());
    // Another receive does not queue behind the blocked one.
    assert!(input.try_recv().is_none());
    assert!(at.elapsed() < Duration::from_millis(40));
    input.close();
    assert!(matches!(
        rx.recv_timeout(WAKE).expect("woke"),
        Err(Error::Disconnected)
    ));
    t.join().unwrap();
}
