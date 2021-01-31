use dbus::Path;
use std::fmt::{self, Display, Formatter};

/// Opaque identifier for a Bluetooth adapter on the system.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdapterId {
    pub(crate) object_path: Path<'static>,
}

impl AdapterId {
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned().into(),
        }
    }
}

impl Display for AdapterId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.object_path
                .to_string()
                .strip_prefix("/org/bluez/")
                .ok_or(fmt::Error)?
        )
    }
}
