use std::io::{self, Read};
use std::ops::{Deref, DerefMut};
use std::panic;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use mio_signals::{Signal, SignalSet, Signals};

#[test]
fn signal_bit_or() {
    // `Signal` and `Signal` (and `Signal`).
    assert_eq!(
        Signal::Terminate | Signal::Quit | Signal::Interrupt,
        SignalSet::all()
    );
    // `Signal` and `SignalSet`.
    assert_eq!(
        Signal::Terminate | SignalSet::from(Signal::Quit),
        Signal::Terminate | Signal::Quit
    );

    // `SignalSet` and `Signal`.
    assert_eq!(
        SignalSet::from(Signal::Interrupt) | Signal::Quit,
        Signal::Quit | Signal::Interrupt
    );
    // `SignalSet` and `SignalSet`.
    assert_eq!(
        SignalSet::from(Signal::Interrupt) | SignalSet::from(Signal::Terminate),
        Signal::Interrupt | Signal::Terminate
    );

    // Overwriting.
    let signal = Signal::Terminate; // This is done to avoid a clippy warning.
    assert_eq!(signal | Signal::Terminate, Signal::Terminate.into());
    assert_eq!(Signal::Terminate | SignalSet::all(), SignalSet::all());
    assert_eq!(SignalSet::all() | Signal::Quit, SignalSet::all());
    assert_eq!(SignalSet::all() | SignalSet::all(), SignalSet::all());
}

#[test]
fn signal_set() {
    let tests = vec![
        (
            SignalSet::all(),
            3,
            vec![Signal::Interrupt, Signal::Terminate, Signal::Quit],
            "Interrupt|Quit|Terminate",
        ),
        (
            Signal::Interrupt.into(),
            1,
            vec![Signal::Interrupt],
            "Interrupt",
        ),
        (
            Signal::Terminate.into(),
            1,
            vec![Signal::Terminate],
            "Terminate",
        ),
        (Signal::Quit.into(), 1, vec![Signal::Quit], "Quit"),
        (
            Signal::Interrupt | Signal::Terminate,
            2,
            vec![Signal::Interrupt, Signal::Terminate],
            "Interrupt|Terminate",
        ),
        (
            Signal::Interrupt | Signal::Quit,
            2,
            vec![Signal::Interrupt, Signal::Quit],
            "Interrupt|Quit",
        ),
        (
            Signal::Terminate | Signal::Quit,
            2,
            vec![Signal::Terminate, Signal::Quit],
            "Quit|Terminate",
        ),
        (
            Signal::Interrupt | Signal::Terminate | Signal::Quit,
            3,
            vec![Signal::Interrupt, Signal::Terminate, Signal::Quit],
            "Interrupt|Quit|Terminate",
        ),
    ];

    for (set, size, expected, expected_fmt) in tests {
        let set: SignalSet = set;
        assert_eq!(set.len(), size);

        // Test `contains`.
        let mut contains_iter = (&expected).iter().cloned();
        while let Some(signal) = contains_iter.next() {
            assert!(set.contains(signal));
            assert!(set.contains::<SignalSet>(signal.into()));

            // Set of the remaining signals.
            let mut contains_set: SignalSet = signal.into();
            for signal in contains_iter.clone() {
                contains_set = contains_set | signal;
            }
            assert!(set.contains(contains_set));
        }

        // Test `SignalSetIter`.
        assert_eq!(set.into_iter().len(), size);
        assert_eq!(set.into_iter().count(), size);
        assert_eq!(set.into_iter().size_hint(), (size, Some(size)));
        let signals: Vec<Signal> = set.into_iter().collect();
        assert_eq!(signals.len(), expected.len());
        for expected in expected {
            assert!(signals.contains(&expected));
        }

        let got_fmt = format!("{:?}", set);
        let got_iter_fmt = format!("{:?}", set.into_iter());
        assert_eq!(got_fmt, expected_fmt);
        assert_eq!(got_iter_fmt, expected_fmt);
    }
}

#[test]
fn signal_set_iter_length() {
    let set = Signal::Interrupt | Signal::Terminate | Signal::Quit;
    let mut iter = set.into_iter();

    assert!(iter.next().is_some());
    assert_eq!(iter.len(), 2);
    assert_eq!(iter.size_hint(), (2, Some(2)));

    assert!(iter.next().is_some());
    assert_eq!(iter.len(), 1);
    assert_eq!(iter.size_hint(), (1, Some(1)));

    assert!(iter.next().is_some());
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.size_hint(), (0, Some(0)));

    assert!(iter.next().is_none());
}

#[test]
fn receive_no_signal() {
    let mut signals = Signals::new(SignalSet::all()).expect("unable to create Signals");
    assert_eq!(signals.receive().expect("unable to receive signal"), None);
}

#[test]
fn example() {
    let child = run_example("signal_handling");

    // Give the process some time to startup.
    sleep(Duration::from_millis(200));

    let pid = child.id() as libc::pid_t;

    send_signal(pid, libc::SIGINT);
    send_signal(pid, libc::SIGQUIT);
    send_signal(pid, libc::SIGTERM);

    let output = read_output(child);
    let want = format!("Call `kill -s TERM {}` to stop the process\nGot interrupt signal\nGot quit signal\nGot terminate signal\n", pid);
    assert_eq!(output, want);
}

/// Wrapper around a `command::Child` that kills the process when dropped, even
/// if the test failed. Sometimes the child command would survive the test when
/// running then in a loop (e.g. with `cargo watch`). This caused problems when
/// trying to bind to the same port again.
struct ChildCommand {
    inner: Child,
}

impl Deref for ChildCommand {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.inner
    }
}

impl DerefMut for ChildCommand {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.inner
    }
}

impl Drop for ChildCommand {
    fn drop(&mut self) {
        let _ = self.inner.kill();
        self.inner.wait().expect("can't wait on child process");
    }
}

/// Run an example, not waiting for it to complete, but it does wait for it to
/// be build.
fn run_example(name: &'static str) -> ChildCommand {
    build_example(name);
    start_example(name)
}

/// Build the example with the given name.
fn build_example(name: &'static str) {
    let output = Command::new("cargo")
        .args(&["build", "--example", name])
        .output()
        .expect("unable to build example");

    if !output.status.success() {
        panic!(
            "failed to build example: {}\n\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Start and already build example
fn start_example(name: &'static str) -> ChildCommand {
    Command::new(format!("target/debug/examples/{}", name))
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map(|inner| ChildCommand { inner })
        .expect("unable to run example")
}

fn send_signal(pid: libc::pid_t, signal: libc::c_int) {
    if unsafe { libc::kill(pid, signal) } == -1 {
        let err = io::Error::last_os_error();
        panic!("error sending signal: {}", err);
    }
}

/// Read the standard output of the child command.
fn read_output(mut child: ChildCommand) -> String {
    child.wait().expect("error running example");

    let mut stdout = child.stdout.take().unwrap();
    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .expect("error reading output of example");
    output
}
