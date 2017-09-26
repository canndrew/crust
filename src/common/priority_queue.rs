struct QueueInner {
    queue: BTreeMap<u8, VecDequeue<(Instant, Vec<u8>)>>,
    sender: mpsc::Sender<()>,
    receiver: mpsc::Reciever<()>,
    buffered: usize,
    capacity: usize,
}

pub fn priority_channel() -> (PrioritySender, PriorityReceiver) {
    let (tx, rx) = mpsc::unbounded();
    let queue_inner = {
        queue: BTreeMap::new(),
        sender: tx,
        receiver: rx,
        buffered: 0,
        capacity: 1024 * 1024,
    };
    let (sender_lock, receiver_lock) = BiLock::new(queue_inner);
    let sender = PrioritySender {
        inner: sender_lock,
    };
    let receiver = PriorityReceiver {
        inner: receiver_lock,
    };
    (sender, receiver)
}

pub struct PrioritySender {
    inner: BiLock<QueueInner>,
}

pub struct PriorityReciever {
    inner: BiLock<QueueInner>,
}

impl Stream for PriorityReceiver {
    type Item = Vec<u8>;
    type Error = Void;

    fn poll(&mut self) -> Result<Async<Option<Vec<u8>>>, Void> {
        if let Async::Ready(mut inner) = self.inner.poll_lock() {
            return match inner.receiver.poll() {
                Ok(Async::Ready(Some(()))) => {
                    let (priority, queue) = unwrap!(inner.queue.pop());
                    let (_, data) = unwrap!(queue.pop());
                    if !queue.is_empty() {
                        inner.queue.insert(priority, queue);
                    }
                    inner.bufferd -= data.len();

                    Ok(Async::Ready(Some(data)))
                },
                Ok(Async::Ready(None)) => Ok(Async::Ready(None))
                Ok(Async::NotReady) => Ok(Async::NotReady),
                Err(()) => unreachable!(),
            }
        }
        Ok(Async::NotReady)
    }
}

impl Sink for PrioritySender {
    type SinkItem = (Vec<u8>, u8);
    type SinkError = Void;

    fn start_send(&mut self, (data, priority): (Vec<u8>, u8)) -> Result<AsyncSink<(Vec<u8>, u8)>, Void> {
        if let Async::Ready(mut inner) = self.inner.poll_lock() {
            let now = Instant::now();
            if data.len() + self.buffered > self.capacity {
                // This is where we drop old messages if our buffer has gotten too large. This is
                // different from the old logic but will have more consistent latency. Basically,
                // for every message in the queue we look at its (age * priority) and drop the
                // message with this highest value (priority is backwards with 0 being the highest
                // priority). We keep dropping messages until we have room for our new message.

                // Collect the fronts of the queues (since these are the oldest messages), order
                // them by (age * priority).
                let mut sub_queue_times = BTreeMap::new();
                for (priority, sub_queue) in inner.queue.iter() {
                    if let Some(&(time, _)) = sub_queue.front() {
                        let age = now - time;
                        sub_queue_times.insert(priority * age, priority);
                    }
                }

                // Repeatedly drop the least important message.
                while self.buffered > 0 && data.len() + self.buffered > self.capacity {
                    let &(key, _) = unwrap!(sub_queue_times.iter().next_back());
                    let (_, priority) = unwrap!(sub_queue_times.remove(&key));
                    let sub_queue = unwrap!(inner.queue.remove(priority));
                    let (_, drop_data) = unwrap!(sub_queue.pop_front());
                    if let Some(&(time, _)) = sub_queue.front() {
                        let age = now - time;
                        sub_queue_times.insert(priority * age, priority);
                        inner.queue.insert(priority, sub_queue);
                    }
                    inner.buffered -= drop_data.len();
                    match inner.receiver.poll() {
                        Ok(Async::Ready(())) => (),
                        _ => unreachable!(),
                    }
                    info!("Insufficient bandwidth. Dropping message of len {}, priority {}",
                          drop_data.len(), priority);
                }
            }

            self.buffered += data.len();
            let mut queue = inner.queue.entry(priority).or_insert(Vec::new());
            queue.push_back((now, item));
            let _ = sender.send(());
            return Ok(AsyncSink::Ready);
        }
        Ok(AsyncSink::NotReady((item, priority)))
    }

    fn poll_complete(&mut self) -> Result<Async<()>, Void> {
        Ok(Async::Ready(()))
    }
}

