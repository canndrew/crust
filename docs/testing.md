# Ideas for testing

This document outlines some ideas for testing, specifically related to Crust.

## Isolated network simulator library

Our current test suites for Crust and p2p are woefully inadequate. For
instance, there isn't (currently) any way to test whether our hole-punching
code actually works at all without having two people manually test it over the
internet. Nor is there any way to automatically test whether Crust can
correctly handle packet loss, packet reordering etc. Luckily there's a solution
to all this.

It's possible to write a Rust library that allows launching a thread into its
own isolated network environment where it only has access to a virtual network
interface. From outside the thread we can then read/write packets directly
to/from this interface in order to introduce latency, packet loss, or any other
form of communications-mangling. Using iptables we can also set up these
threads with their own NAT rules.

To showcase what I'm talking about, we could use this library to write test
cases that look something like this:

```rust
// Example Crust test
#[test]
fn test_connect_to_peers() {
    let (tx, rx) = mpsc::oneshot();

    internet_simulator::run(&[
        || {
            let service = Service::new();
            service.start_listening();
            let addr = expect_event!(service, Event::ListenerStarted(addr) => addr);
            tx.send(addr);
            let peer = expect_event!(service, Event::BootstrapAccept(peer) => peer);

            // we connected! now exchange data or something.
        },
        || {
            let service = Service::new();
            let addr = rx.recv();
            service.add_contact(addr);
            service.bootstrap();
            let peer = expect_event!(service, Event::BootstrapConnect(peer) => peer);

            // we connected! now exchange data or something.
        },
    ]);
}
```

In the above code, the two Crust services would each have their own IP address,
be behind their own NAT, and be communicating over a lossy connection with
latency. This would be a *much* more effective test than what we currently do
in Crust (which is just have peers talk to each other over loopback).

Once we can run Crust nodes in their own isolated environments like this we
would also be able to do away with mock-crust in routing. Test cases could
simply set up their own virtual networks like this.

I have already have a basic proof-of-concept of this working. It's Linux-only
and isn't yet fully developed so you can't spin up a virtual internet with a
single command, but all the essentials are in-place. It just needs man-hours
put into it.

## Router probing tool

Routers in the wild exhibit all kind of wierd and wacky behaviours which effect
our ability to hole-punch and reliably maintain a connection. It would be good
if we could catalogue these behaviours and their prevalence. This would help us
know how Crust could be improved and how urgently certain improvements may be
needed.

A way to do this cataloguing would be to release a tool to our users which runs
a set of tests while communicating with a set of external servers, then reports
the test results back to us. Eventually, some form of this test could be
integrated into the p2p library in order to avoid the need for the user to
do their own manual configuration.

## Automated soak testing

When testing uTP I found bugs that would only start occuring after sending
hundreds of megabytes over a connection. It's not possible to test for these
kinds of bugs in our current automated tests because these tests need to be
fairly fast - we don't want to lock-up Travis for several hours every time we
push a commit. However it would be good if we had *some* way to automatically
run tests like this.

My suggestion is to add a different kind of test to our crates, labelled
`#[soak_test]`, which are designed to run forever. We then set-aside a machine
somewhere, presumably in the MaidSAFE office, which automatically stays
up-to-date with master on all our github repositories, finds these tests, and
keeps them running 24/7.

