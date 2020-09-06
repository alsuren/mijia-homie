use bluez_generated::bluetooth_event::BluetoothEvent;
use core::fmt::Debug;
use core::future::Future;
use dbus::{nonblock::SyncConnection, Message};
use futures::{FutureExt, Stream, StreamExt};
use std::sync::Arc;

// TODO before publishing to crates.io: annotate this enum as non-exhaustive.
#[derive(Clone)]
pub enum MijiaEvent {
    // FIXME: stop using object_path as primary key. Can we think of something better?
    Readings {
        object_path: String,
        readings: crate::Readings,
    },
    Disconnected {
        object_path: String,
    },
}

impl MijiaEvent {
    fn from(conn_msg: Message) -> Option<Self> {
        match BluetoothEvent::from(conn_msg) {
            Some(BluetoothEvent::Value { object_path, value }) => {
                // TODO: Make this less hacky.
                let object_path = object_path
                    .strip_suffix(crate::SERVICE_CHARACTERISTIC_PATH)?
                    .to_owned();
                let readings = crate::decode_value(&value)?;
                Some(MijiaEvent::Readings {
                    object_path,
                    readings,
                })
            }
            Some(BluetoothEvent::Connected {
                object_path,
                connected: false,
            }) => Some(MijiaEvent::Disconnected { object_path }),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct MijiaSession {
    pub connection: Arc<SyncConnection>,
}

impl Debug for MijiaSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MijiaSession")
    }
}

impl MijiaSession {
    /// Returns a tuple of (join handle, Self).
    /// If the join handle ever completes then you're in trouble and should
    /// probably restart the process.
    pub async fn new() -> Result<
        (
            impl Future<Output = Result<(), anyhow::Error>>,
            MijiaSession,
        ),
        anyhow::Error,
    > {
        // Connect to the D-Bus system bus (this is blocking, unfortunately).
        let (dbus_resource, connection) = dbus_tokio::connection::new_system_sync()?;
        // The resource is a task that should be spawned onto a tokio compatible
        // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
        let dbus_handle = tokio::spawn(async {
            let err = dbus_resource.await;
            // TODO: work out why this err isn't 'static and use anyhow::Error::new instead
            Err::<(), anyhow::Error>(anyhow::anyhow!(err))
        });
        Ok((
            dbus_handle.map(|res| Ok(res??)),
            MijiaSession { connection },
        ))
    }

    pub async fn event_stream(self) -> Result<impl Stream<Item = MijiaEvent>, anyhow::Error> {
        let mut rule = dbus::message::MatchRule::new();
        rule.msg_type = Some(dbus::message::MessageType::Signal);
        rule.sender =
            Some(dbus::strings::BusName::new("org.bluez").map_err(|s| anyhow::anyhow!(s))?);

        // TODO: run this in a scope guard or something when the event stream is dropped:
        //     self.connection.remove_match(msg_match.token()).await?;
        let (_msg_match, events) = self.connection.add_match(rule).await?.msg_stream();

        Ok(Box::pin(events.filter_map(|event| async move {
            MijiaEvent::from(event)
        })))
    }
}
