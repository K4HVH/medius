//! A control port another process holds is refused at open, naming the holder: two readers split
//! the box's frames between them.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

// A pty whose master another process keeps open (the crate forbids unsafe code, so no openpty
// here). Its slave is left unheld; the caller decides who holds it.
fn pty() -> (Owner, String) {
    let mut owner = Command::new("python3")
        .args([
            "-c",
            "import os, pty, select, sys, time\n\
             m, s = pty.openpty()\n\
             print(os.ttyname(s), flush=True)\n\
             os.close(s)\n\
             sys.stdin.readline()\n\
             ready = select.select([m], [], [], 0.2)[0]\n\
             print(len(os.read(m, 4096)) if ready else 0, flush=True)\n\
             time.sleep(60)",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn python3");
    let mut out = BufReader::new(owner.stdout.take().expect("stdout"));
    let mut name = String::new();
    out.read_line(&mut name).expect("read the pty name");
    (Owner { child: owner, out }, name.trim().to_string())
}

struct Owner {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
}

impl Owner {
    // The bytes written to the slave so far, read off the master.
    fn bytes_received(&mut self) -> usize {
        writeln!(self.child.stdin.as_mut().expect("stdin")).expect("ask");
        let mut line = String::new();
        self.out.read_line(&mut line).expect("read the count");
        line.trim().parse().expect("a count")
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn port_busy_names_holder() {
    let (_owner, slave) = pty();
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(File::open(&slave).expect("open the slave"))
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn sleep");
    std::thread::sleep(Duration::from_millis(100));

    let want = format!("sleep (pid {})", child.id());
    match crate::Device::open(&slave) {
        Err(crate::Error::PortBusy { holders }) => {
            assert!(holders.contains(&want), "{holders:?} lacks {want}")
        }
        other => panic!("expected PortBusy, got {other:?}"),
    }
    match crate::Device::open_for_update(&slave) {
        Err(crate::Error::PortBusy { .. }) => {}
        other => panic!("open_for_update: expected PortBusy, got {other:?}"),
    }

    child.kill().expect("kill sleep");
    child.wait().expect("reap sleep");
    let after = crate::Device::open(&slave).err();
    assert!(
        !matches!(after, Some(crate::Error::PortBusy { .. })),
        "still busy after the holder exited: {after:?}"
    );
}

#[test]
fn listing_skips_a_held_port() {
    let (mut owner, slave) = pty();
    let mut child = Command::new("sleep")
        .arg("30")
        .stdin(File::open(&slave).expect("open the slave"))
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn sleep");
    std::thread::sleep(Duration::from_millis(100));
    let port = crate::PortInfo {
        path: slave.clone(),
        vid: 0x1a86,
        pid: 0x55d3,
        serial: None,
    };
    assert!(crate::device::discover::probe(&port).is_none());
    assert_eq!(
        owner.bytes_received(),
        0,
        "the probe wrote to a port another process holds"
    );
    child.kill().expect("kill sleep");
    child.wait().expect("reap sleep");
}
