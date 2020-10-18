# Generated bindings for BlueZ

This crate contains async bindings for Bluez.

Bindings are generated from introspection data, using `dbus-codegen`.
This means that it is relatively easy to maintain, but it only covers interfaces
that I have the devices for.

# Adding Interfaces

If there is an interface that you need which is not generated, it should be
reasonably straightforward to generate them and send a pull request. See
[introspect.sh](https://github.com/alsuren/mijia-homie/blob/master/bluez-generated/introspect.sh)
for details. It's also perfectly reasonable to generate the interfaces you need
and vendor them into your project.

# Future Direction

Only async bindings are generated. Blocking bindings could also be generated,
but I'm unlikely to use them, so they would need to be contributed by someone
else.

It would be nice to generate some strongly typed bindings around
`get_managed_objects()` for bulk-fetching properties and `add_match()` for
subscribing to events.
