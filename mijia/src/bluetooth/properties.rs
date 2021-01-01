use dbus::arg::{prop_cast, RefArg, Variant};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug)]
pub struct OrgBluezDevice1Properties<'a>(pub &'a HashMap<String, Variant<Box<dyn RefArg>>>);

impl<'a> OrgBluezDevice1Properties<'a> {
    pub fn from_interfaces(
        interfaces: &'a HashMap<String, HashMap<String, Variant<Box<dyn RefArg>>>>,
    ) -> Option<Self> {
        interfaces.get("org.bluez.Device1").map(Self)
    }

    pub fn address(&self) -> Option<&String> {
        prop_cast(self.0, "Address")
    }

    pub fn name(&self) -> Option<&String> {
        prop_cast(self.0, "Name")
    }

    pub fn paired(&self) -> Option<bool> {
        prop_cast(self.0, "Paired").copied()
    }

    pub fn service_data(&self) -> Option<&HashMap<String, Variant<Box<dyn RefArg>>>> {
        prop_cast(self.0, "ServiceData")
    }
}
