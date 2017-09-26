trait StreamExt: Stream {
    pub fn timeout_at(self, instant: Instant) -> TimeoutAt<Self> {
        TimeoutAt {
            timeout: Timeout::new_at(instant),
            stream: self,
        }
    }
}

impl<T: Stream> StreamExt for T {}

pub struct TimeoutAt<S> {
    stream: S,
    timeout: Timeout,
}

impl<S> Stream for TimeoutAt<S> {
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Result<Async<Option<S::Item>>, S::Error> {
        if let Async::Ready(()) = unwrap!(self.timeout.poll()) {
            return Ok(Async::Ready(None));
        }

        self.stream.poll()
    }
}

