//! Message passing abstractions for sending notifications to Tokio tasks and
//! awaiting their acknowledgement, which is useful for graceful shutdowns.
//! This is how it works:
//!
//! 1. We crate a new [`Notifier`].
//! 2. The [`Notifier`] can give away multiple [`Subscription`] objects.
//! 3. Each [`Subscription`] can receive notifications from the [`Notifier`].
//! 4. Each [`Subscription`] can acknowledge the last notification it received.
//!
//! In order to perform steps 1, 2 and 3 we need a [`broadcast`] channel where
//! the [`Notifier`] sends messages and subscribers read them, while step 4
//! requires an additional [`mpsc`] channel to send acknowledgements back to the
//! [`Notifier`].

use tokio::sync::{broadcast, mpsc};

/// Message that can be sent as a notification to Tokio tasks.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Notification {
    /// This is used for graceful shutdowns.
    Shutdown,
}

/// Notifier object that can send messages to its subscribers.
pub(crate) struct Notifier {
    /// Sender half of the notifications channel.
    notification_sender: broadcast::Sender<Notification>,

    /// Receiver part of the acknowledgements channel.
    acknowledge_receiver: mpsc::Receiver<()>,

    /// Sender part of the acknowledgements channel. Used only to give away
    /// senders to subscribers so that they can acknowledge receipt of
    /// notifications.
    acknowledge_sender: mpsc::Sender<()>,
}

/// Used by subscribers to obtain a notification from a [`Notifier`] and
/// acknowledge receipt when possible.
pub(crate) struct Subscription {
    /// Receiver half of the notifications channel.
    notification_receiver: broadcast::Receiver<Notification>,

    /// Sender half of the acknowledgements channel.
    acknowledge_sender: mpsc::Sender<()>,
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
    /// [`Subscription`] object that can be used to receive a [`Notification`].
    pub fn subscribe(&self) -> Subscription {
        let notification_receiver = self.notification_sender.subscribe();
        let acknowledge_sender = self.acknowledge_sender.clone();

        Subscription::new(notification_receiver, acknowledge_sender)
    }

    /// Sends a [`Notification`] to all subscribers.
    pub fn send(
        &self,
        notification: Notification,
    ) -> Result<usize, broadcast::error::SendError<Notification>> {
        self.notification_sender.send(notification)
    }

    /// Waits for all the subscribers to acknowledge the last sent
    /// [`Notification`].
    pub async fn collect_acknowledgements(self) {
        let Self {
            notification_sender,
            mut acknowledge_receiver,
            acknowledge_sender,
        } = self;

        // This sender half of a multi producer single consumer channel must
        // dropped before we await on the receiver half down below because the
        // channel is closed only when all senders are dropped. The one that we
        // own is not used for sending, it's used to give away clones.
        drop(acknowledge_sender);

        // Wait for all acks one by one. As stated above, the channel is closed
        // when all senders are dropped, which causes `recv` to return None.
        while let Some(_ack) = acknowledge_receiver.recv().await {
            // Wait for all acks
        }

        // This one is dropped at the end, otherwise receivers will get an
        // error because there are no more senders on the channel.
        drop(notification_sender);
    }
}

impl Subscription {
    /// Creates a new [`Subscription`] object.
    pub fn new(
        notification_receiver: broadcast::Receiver<Notification>,
        acknowledge_sender: mpsc::Sender<()>,
    ) -> Self {
        Self {
            notification_receiver,
            acknowledge_sender,
        }
    }

    /// Reads the notifications channel to check if a notification was sent.
    /// We discard all the errors because none of the can happen unless we
    /// misuse the [`Notifier`] struct:
    ///
    /// - [`broadcast::error::TryRecvError::Closed`]: can't happen because the
    /// [`Notifier`] won't close it's sender half until acks are collected, see
    /// [`Notifier::collect_acknowledgements`]. If that function is not called,
    /// this error is possible and would be discarded.
    ///
    /// - [`broadcast::error::TryRecvError::Lagged`]: can't happen because the
    /// sender will wait for acks before sending new values to the channel. If
    /// the sender doesn't wait, then again, this error is possible.
    ///
    /// - [`broadcast::error::TryRecvError::Empty`]: we don't even care about
    /// empty buffers, if no notification was sent then return `None`.
    pub fn receive_notification(&mut self) -> Option<Notification> {
        self.notification_receiver.try_recv().ok()
    }

    /// Sends an ACK on the acknowledgements channel. For now, acks are not
    /// tied to notifications, so we interpret this as "acknowledge the last
    /// read notification". Errors as discarded here as well:
    ///
    /// - [`mpsc::error::SendError<T>`]: can't happen unless all receivers
    /// closed their channel, which they don't until sending the ack.
    pub async fn acknowledge_notification(&self) {
        self.acknowledge_sender.send(()).await.unwrap();
    }
}
