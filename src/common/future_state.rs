use std::any::Any;
use futures::{Async, Future};
use common::{self, Core, CoreTimer, CoreMessage, State};
use std::rc::Rc;
use std::cell::RefCell;
use std::io;
use std::time::Duration;
use mio::{Evented, Poll, Token, Ready, PollOpt};
use mio::channel::Sender;
use void::Void;

scoped_thread_local!(static CURRENT_TASK: Rc<RefCell<Task>>);

/// Spawn a future on crust's event loop. 
/// Note that any IO performed by these futures must be done using crust's IO types: PollEvented
/// and Timeout. You cannot use tokio types on here.
pub fn spawn_future_async<F>(f: F)
where F: Future<Item=(), Error=Void> + 'static {
    let task = Task::new(f);
    let task = Rc::new(RefCell::new(task));
    run_task(&task);
}

struct Task {
    future: Option<Box<Future<Item=(), Error=Void>>>,
}

impl Task {
    pub fn new<F>(f: F) -> Task
    where F: Future<Item=(), Error=Void> + 'static {
        Task {
            future: Some(Box::new(f)),
        }
    }

    pub fn none() -> Rc<RefCell<Task>> {
        Rc::new(RefCell::new(Task { future: None, }))
    }
}

fn run_task(task: &Rc<RefCell<Task>>) {
    let task_cloned = task.clone();
    let destroy = CURRENT_TASK.set(task, || {
        let mut task_inner = task_cloned.borrow_mut();
        match task_inner.future {
            Some(ref mut f) => match f.poll() {
                Ok(Async::Ready(())) => true,
                Ok(Async::NotReady) => false,
                Err(v) => match v {},
            },
            None => false,
        }
    });
    if destroy {
        let mut task_inner = task.borrow_mut();
        task_inner.future = None;
    }
}

fn get_current_task() -> Rc<RefCell<Task>> {
    CURRENT_TASK.with(|t| t.clone())
}

struct FdState {
    inner: Rc<RefCell<FdStateInner>>,
}

struct FdStateInner {
    read_task: Rc<RefCell<Task>>,
    write_task: Rc<RefCell<Task>>,
    read_ready: bool,
    write_ready: bool,
    sender: Sender<CoreMessage>,
    token: Token,
}

impl State for FdState {
    fn as_any(&mut self) -> &mut Any {
        self
    }

    fn ready(&mut self, _core: &mut Core, _poll: &Poll, kind: Ready) {
        if kind.is_readable() {
            let task = {
                let mut inner = self.inner.borrow_mut();
                inner.read_ready = true;
                inner.read_task.clone()
            };
            run_task(&task);
        }
        if kind.is_writable() {
            let task = {
                let mut inner = self.inner.borrow_mut();
                inner.write_ready = true;
                inner.write_task.clone()
            };
            run_task(&task);
        }
    }
}

/// Wraps an `Evented` and allows us to poll it.
/// Intended to be similar to the tokio PollEvented type.
pub struct PollEvented<E> {
    evented: E,
    read_ready: bool,
    write_ready: bool,
    inner: Rc<RefCell<FdStateInner>>,
}

impl<E> PollEvented<E>
where E: Evented {
    /// Create a new `PollEvented`
    pub fn new(evented: E, core: &mut Core, poll: &Poll) -> io::Result<PollEvented<E>> {
        let token = core.get_new_token();
        poll.register(&evented, token, Ready::readable() | Ready::writable(), PollOpt::edge())?;
        let inner = Rc::new(RefCell::new(FdStateInner {
            read_task: Task::none(),
            write_task: Task::none(),
            read_ready: false,
            write_ready: false,
            sender: core.sender().clone(),
            token: token,
        }));
        let fd_state = Rc::new(RefCell::new(FdState {
            inner: inner.clone(),
        }));
        let _  = core.insert_state(token, fd_state);
        Ok(PollEvented {
            evented: evented,
            read_ready: false,
            write_ready: false,
            inner: inner,
        })
    }
    
    /// Check whether the `PollEvented` is ready for reading.
    pub fn poll_read(&mut self) -> Async<()> {
        if self.read_ready {
            return Async::Ready(());
        }

        let mut inner = self.inner.borrow_mut();
        if inner.read_ready {
            inner.read_ready = false;
            self.read_ready = true;
            Async::Ready(())
        } else {
            inner.read_task = get_current_task();
            Async::NotReady
        }
    }

    /// Tell the event loop that this `Evented` is waiting on Ready::readable()
    /// Registers the current task to run when it's ready.
    pub fn need_read(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.read_task = get_current_task();
        self.read_ready = false;
    }
    
    /// Check whether the `PollEvented` is ready for writing.
    pub fn poll_write(&mut self) -> Async<()> {
        if self.write_ready {
            return Async::Ready(());
        }

        let mut inner = self.inner.borrow_mut();
        if inner.write_ready {
            inner.write_ready = false;
            self.write_ready = true;
            Async::Ready(())
        } else {
            inner.write_task = get_current_task();
            Async::NotReady
        }
    }

    /// Tell the event loop that this `Evented` is waiting on Ready::writable()
    /// Registers the current task to run when it's ready.
    pub fn need_write(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.write_task = get_current_task();
        self.write_ready = false;
    }

    /// Get a reference to the inner `Evented`
    pub fn get_ref(&self) -> &E {
        &self.evented
    }
}

impl<E> Drop for PollEvented<E> {
    fn drop(&mut self) {
        let inner = self.inner.borrow();
        let token = inner.token;
        let _ = inner.sender.send(CoreMessage::new(move |core, _poll| {
            let _ = core.remove_state(token);
        }));
    }
}

struct TimeoutState {
    inner: Rc<RefCell<TimeoutStateInner>>,
}

struct TimeoutStateInner {
    task: Rc<RefCell<Task>>,
    token: Token,
    triggered: bool,
    sender: Sender<CoreMessage>,
}

impl State for TimeoutState {
    fn as_any(&mut self) -> &mut Any {
        self
    }

    fn timeout(&mut self, _core: &mut Core, _poll: &Poll, _timer_id: u8) {
        let task = {
            let mut inner = self.inner.borrow_mut();
            inner.triggered = true;
            inner.task.clone()
        };
        run_task(&task);
    }
}

/// A future which finishes after the given duration.
pub struct Timeout {
    inner: Rc<RefCell<TimeoutStateInner>>,
}

impl Timeout {
    /// Create a `Timeout` which finishes after the given duration.
    pub fn new(duration: Duration, core: &mut Core) -> common::Result<Timeout> {
        let token = core.get_new_token();
        let sender = core.sender().clone();
        let inner = Rc::new(RefCell::new(TimeoutStateInner {
            task: Task::none(),
            token: token,
            triggered: false,
            sender: sender,
        }));
        let state = Rc::new(RefCell::new(TimeoutState {
            inner: inner.clone(),
        }));
        let _ = core.insert_state(token, state);
        let core_timer = CoreTimer {
            state_id: token, 
            timer_id: 0,
        };
        let _ = core.set_timeout(duration, core_timer)?;
        Ok(Timeout {
            inner: inner,
        })
    }
}

impl Future for Timeout {
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> io::Result<Async<()>> {
        let mut inner = self.inner.borrow_mut();
        if inner.triggered {
            Ok(Async::Ready(()))
        } else {
            inner.task = get_current_task();
            Ok(Async::NotReady)
        }
    }
}

impl Drop for Timeout {
    fn drop(&mut self) {
        let inner = self.inner.borrow();
        let token = inner.token;
        let _ = inner.sender.send(CoreMessage::new(move |core, _poll| {
            let _ = core.remove_state(token);
        }));
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use common::{self, CoreMessage};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tests::timebomb;
    use mio::Registration;
    use futures::future;

    #[test]
    fn test_poll_evented_and_timeouts() {
        let timeout_duration = Duration::new(1, 0);
        timebomb(timeout_duration * 3, || {
            let start_time = Instant::now();
            let el = unwrap!(common::spawn_event_loop(0, Some("wow")));
            let (tx, rx) = mpsc::channel();
            unwrap!(el.send(CoreMessage::new(move |core, poll| {
                // Spawn two futures and run them to complettion.
                // One future uses timeouts, the other uses a PollEvented wrapped around a
                // mio::Registration.
                

                let (registration, set_readiness) = Registration::new2();

                // The first future raises mio events on the second future after 1 and 2 seconds.
                let first_timeout = unwrap!(Timeout::new(timeout_duration, core));
                let second_timeout = unwrap!(Timeout::new(timeout_duration * 2, core));
                let timer_future = first_timeout
                    .and_then(move |()| {
                        unwrap!(set_readiness.set_readiness(Ready::readable()));
                        second_timeout.map(move |()| {
                            unwrap!(set_readiness.set_readiness(Ready::writable()));
                        })
                    }).map_err(|e| panic!("error: {:?}", e));
                spawn_future_async(timer_future);


                // The second future waits for it's mio events and reports the times that they
                // occured on it's Sender.
                let mut poll_evented = unwrap!(PollEvented::new(registration, core, poll));
                let mut already_done_read = false;
                let evented_future = future::poll_fn(move || {
                    if let Async::Ready(()) = poll_evented.poll_read() {
                        assert!(!already_done_read);
                        already_done_read = true;
                        unwrap!(tx.send(Instant::now()));
                        poll_evented.need_read();
                    }

                    if let Async::Ready(()) = poll_evented.poll_write() {
                        assert!(already_done_read);
                        unwrap!(tx.send(Instant::now()));
                        poll_evented.need_write();
                        return Ok(Async::Ready(()));
                    }

                    Ok(Async::NotReady)
                });
                spawn_future_async(evented_future);
            })));

            let approx_eq = |instant0, instant1| -> bool {
                let diff = if instant0 < instant1 {
                    instant1 - instant0
                } else {
                    instant0 - instant1
                };
                diff < Duration::from_millis(10)
            };

            // Make sure that we responded to the mio events at the right times.

            let time0 = unwrap!(rx.recv());
            assert!(approx_eq(time0, start_time + timeout_duration));

            let time1 = unwrap!(rx.recv());
            assert!(approx_eq(time1, start_time + timeout_duration * 2));

            if let Ok(time2) = rx.recv() {
                panic!("There shouldn't be a time2! {:?}", time2);
            }
        });
    }
}

