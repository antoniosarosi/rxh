//! Message passing abstractions for sending notifications to Tokio tasks and
//! awaiting their acknowledgement, which is useful for gracefull shutdowns.

use tokio::sync::{broadcast, mpsc};

/// Message that can be sent as a notification to Tokio tasks.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Message {
    /// This is used for graceful shutdowns.
    Shutdown,
}

/// Notifier object that can send messages to its subscribers.
pub(crate) struct Notifier {
    /// Sender half of the broadcast channel used to notify subscribers.
    notification_sender: broadcast::Sender<Message>,

    /// Receiver part of the acknowledgements channel.
    acknowledge_receiver: mpsc::Receiver<()>,

    /// Sender part of the acknowledgements channel. Used only to give away
    /// senders to subscribers.
    acknowledge_sender: mpsc::Sender<()>,
}

/// Used by subscribers to obtain a notification from a [`Notifier`] and
/// acknowledge receipt when possible.
pub(crate) struct Notification {
    /// Receiver half of the notifications channel.
    receiver: broadcast::Receiver<Message>,

    /// Sender half of the acknowledgements channel.
    acknowledge: mpsc::Sender<()>,
}

impl Notifier {
    /// Creates a new [`Notifier`] with all the channels set up.
    pub fn new() -> Self {
        let (notification_sender, _) = broadcast::channel(1);
        let (acknowledge_sender, acknowledge_receiver) = mpsc::channel(1);

        Self {
            notification_sender,
            acknowledge_sender,
            acknowledge_receiver,
        }
    }

    /// By subscribing to this [`Notifier`] the caller obtains a
    /// [`Notification`] object that can be used to receive messages.
    pub fn subscribe(&self) -> Notification {
        let receive = self.notification_sender.subscribe();
        let acknowledge = self.acknowledge_sender.clone();

        Notification::new(receive, acknowledge)
    }

    /// Sends a message to all subscribers.
    pub fn send(&self, message: Message) -> Result<usize, broadcast::error::SendError<Message>> {
        self.notification_sender.send(message)
    }

    /// Waits for all the subscribers to acknowledge the last sent message.
    pub async fn collect_acknowledgements(self) {
        let Self {
            notification_sender,
            mut acknowledge_receiver,
            acknowledge_sender,
        } = self;

        // This sender must be dropped before we await acks, since the receiver
        // future at the bottom of this function would never finish otherwise.
        // Receivers only finish when all senders are dropped.
        drop(acknowledge_sender);

        // This one is not so important but we don't need it anymore.
        drop(notification_sender);

        // We don't care if the channel is closed, that means that all senders
        // have been dropped, either by successfully fullfilling requests or
        // not, but it doesn't matter.
        acknowledge_receiver.recv().await;
    }
}

impl Notification {
    pub fn new(receiver: broadcast::Receiver<Message>, acknowledge: mpsc::Sender<()>) -> Self {
        Self {
            receiver,
            acknowledge,
        }
    }

    // Can't receive lagged error because the channel has capacity 1, we
    // can't receive closed error either because the sender won't drop its
    // part until all acks have been received. Finally, we don't care about
    // empty buffers, we just want to know if we should notify completion of
    // a task.
    pub fn receive(&mut self) -> Option<Message> {
        self.receiver.try_recv().ok()
    }

    pub async fn acknowledge(&self) {
        // Receiver can't be closed when we send acks, so no error is possible.
        let _ = self.acknowledge.send(()).await;
    }
}

impl Clone for Notification {
    fn clone(&self) -> Self {
        Self {
            receiver: self.receiver.resubscribe(),
            acknowledge: self.acknowledge.clone(),
        }
    }
}
