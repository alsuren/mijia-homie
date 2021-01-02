// This code was autogenerated with `dbus-codegen-rust --file=specs/org.bluez.GattManager1.xml --interfaces=org.bluez.GattManager1 --client=nonblock --methodtype=none --prop-newtype`, see https://github.com/diwic/dbus-rs
#[allow(unused_imports)]
use dbus::arg;
use dbus::nonblock;

pub const ORG_BLUEZ_GATT_MANAGER1_NAME: &str = "org.bluez.GattManager1";

pub trait OrgBluezGattManager1 {
    fn register_application(
        &self,
        application: dbus::Path,
        options: ::std::collections::HashMap<&str, arg::Variant<Box<dyn arg::RefArg>>>,
    ) -> nonblock::MethodReply<()>;
    fn unregister_application(&self, application: dbus::Path) -> nonblock::MethodReply<()>;
}

impl<'a, T: nonblock::NonblockReply, C: ::std::ops::Deref<Target = T>> OrgBluezGattManager1
    for nonblock::Proxy<'a, C>
{
    fn register_application(
        &self,
        application: dbus::Path,
        options: ::std::collections::HashMap<&str, arg::Variant<Box<dyn arg::RefArg>>>,
    ) -> nonblock::MethodReply<()> {
        self.method_call(
            "org.bluez.GattManager1",
            "RegisterApplication",
            (application, options),
        )
    }

    fn unregister_application(&self, application: dbus::Path) -> nonblock::MethodReply<()> {
        self.method_call(
            "org.bluez.GattManager1",
            "UnregisterApplication",
            (application,),
        )
    }
}
