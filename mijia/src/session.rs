use crate::{decode_value, Readings, DBUS_METHOD_CALL_TIMEOUT, SENSOR_READING_CHARACTERISTIC_PATH};
use anyhow::Context;
use bluez_generated::bluetooth_event::BluetoothEvent;
use bluez_generated::generated::{OrgBluezAdapter1, OrgBluezDevice1};
use core::fmt::Debug;
use core::future::Future;
use core::time::Duration;
use dbus::{
    nonblock::{MsgMatch, SyncConnection},
    Message,
};
use futures::{FutureExt, Stream, StreamExt};
use std::sync::Arc;

// TODO before publishing to crates.io: annotate this enum as non-exhaustive.
#[derive(Clone)]
pub enum MijiaEvent {
    // FIXME: stop using object_path as primary key. Can we think of something better?
    Readings {
        object_path: String,
        readings: Readings,
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
                    .strip_suffix(SENSOR_READING_CHARACTERISTIC_PATH)?
                    .to_owned();
                let readings = decode_value(&value)?;
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

    pub async fn start_discovery(&self) -> Result<(), anyhow::Error> {
        let adapter = dbus::nonblock::Proxy::new(
            "org.bluez",
            "/org/bluez/hci0",
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        );
        adapter
            .set_powered(true)
            .await
            .with_context(|| std::line!().to_string())?;
        adapter
            .start_discovery()
            .await
            .unwrap_or_else(|err| println!("starting discovery failed {:?}", err));
        Ok(())
    }

    /// Get a stream of reading/disconnected events for all sensors.
    ///
    /// If the MsgMatch is dropped then the Stream will close.
    pub async fn event_stream(
        &self,
    ) -> Result<(MsgMatch, impl Stream<Item = MijiaEvent>), anyhow::Error> {
        let mut rule = dbus::message::MatchRule::new();
        rule.msg_type = Some(dbus::message::MessageType::Signal);
        rule.sender =
            Some(dbus::strings::BusName::new("org.bluez").map_err(|s| anyhow::anyhow!(s))?);

        let (msg_match, events) = self.connection.add_match(rule).await?.msg_stream();

        Ok((
            msg_match,
            Box::pin(events.filter_map(|event| async move { MijiaEvent::from(event) })),
        ))
    }

    fn device(&self, object_path: &str) -> impl OrgBluezDevice1 {
        dbus::nonblock::Proxy::new(
            "org.bluez",
            object_path.to_owned(),
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        )
    }

    pub async fn connect(&self, object_path: &str) -> Result<(), anyhow::Error> {
        self.device(object_path)
            .connect()
            .await
            .with_context(|| std::line!().to_string())
    }

    pub async fn disconnect(&self, object_path: &str) -> Result<(), anyhow::Error> {
        self.device(object_path)
            .disconnect()
            .await
            .with_context(|| std::line!().to_string())
    }
}
