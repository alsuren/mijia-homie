# Monitoring Temperature

(with too many Bluetooth thermometers)

![](./title.jpg)

David Laban, Andrew Walbran, Anusha Ramdarshan

---

# Outline

- Backstory.

- System Overview.

- How it's built.

- Concurrency pitfalls.

- Observations about the project.

- Hall of Fame

- Links and Questions.

---

# Backstory

- We started with a few ESP32 sensors like this:
  ![](./inception-yun_hat_04.jpg)
- These cost around US$16 each, and Andrew couldn't get them running more than about a day on battery
  power.

---

# Backstory

- "Wouldn't it be nice to have a hundred of these?"

- "Just imagine what you could do."

- "What's the cheapest way to do this?"

---

# Backstory

- So we bought some of these, at $3 each.

![](./inception-order.png)

---

# Backstory

- And now we have graphs like this:

![](./grafana-temperature.png)

---

# Backstory

- And this:

![](./grafana-fridge.png)

---

# System Overview

- This is what it looks like:
  ![](./system-overview.svg)
- Orange is our code.
  <!-- TODO: slide at the end that describes Will's setup -->
  <!-- TODO: slide at the end that describes cloudbbq-homie -->

--

- Let's dig into the different pieces.

---

# Rust

- Picked because of a
  [blog post](https://dev.to/lcsfelix/using-rust-blurz-to-read-from-a-ble-device-gmb) that David
  found.

- Rust is probably not the **best** language for this.

  - Bluetooth stack on Linux is quite dynamic in places.

  - Cross-compiling with `cross` is okay to set up, but a bit slow.

  - We found a [Python project](https://github.com/JsBergbau/MiTemperature2) partway through, with
    similar objectives.

<!-- prettier-ignore-start -->
<!--
  Indentation is bigger here because that's how much indentation remark needs
  to render a third-level bullet point.
  Ideally I would set tab width to 4 everywhere in prettier, but that makes
  prettier do strange things (https://github.com/prettier/prettier/issues/5019).
-->

<!-- TODO: split this slide at this point? -->

- It was fun anyway:

    - Good chance to work on something together during lockdown.

    - We're both starting to use Rust for work, so good for learning.

        - Andrew is working on
          [crosvm](https://chromium.googlesource.com/chromiumos/platform/crosvm/) and
          [Virt Manager](https://android.googlesource.com/platform/packages/modules/Virtualization/+/refs/heads/master/virtmanager/)
          for Android.

        - David was using Rust for the backend of
          [FutureNHS](https://github.com/FutureNHS/futurenhs-platform/).

<!-- prettier-ignore-end -->

---

# MQTT

- MQTT is the pubsub of choice for low-powered gadgets.

- Has `retain`ed messages:

  - Lets you get the current status from the broker.

  - Avoids a round-trip to a power/network-constrained device.

- Has `LastWill` messages:

  - Lets the server clean up after you when you drop off the network.

- [Homie](https://homieiot.github.io/) is an auto-discovery convention built on MQTT.

- `rumqttc` library is pretty good:

  - Works using channels, which is nice.

  - You are responsible for polling its event loop.

  - Andrew has submitted patches, and they were well received.

---

# Bluetooth

<!-- TODO: make this into a thin summary slide and move interesting content to new slides -->

The Rust Bluetooth story is a bit sad.

- `blurz` - "Bluetooth from before there was Tokio"
  - We started with this.
  - Talks to BlueZ over D-Bus, but single-threaded and synchronous.
  - Blocking `device.connect()` calls. 😧
  - Unmaintained (for 2 years).

<!-- prettier-ignore-start -->

- `btleplug` - "cross-platform jumble"
  - Theoretically cross platform, but many features not implemented.
  - Linux implementation talked to kernel directly over raw sockets, bypassing BlueZ daemon.
      - Requires extra permissions, adds extra bugs.
      - This has since been changed.
  - Tried switching to this (but gave up after too many panicking threads).
  - Andrew now working to improve it and make it async.
  <!-- TODO: potentially re-write this section or split half of it out into its own slide? -->

<!-- prettier-ignore-end -->

- `dbus-rs` - "roll your own BlueZ wrapper"
  - Generates code from D-Bus introspection.
  - Single-threaded because return types are !Send (but that's okay).
  - Async or blocking.

---

# Bluetooth

We ended up building our own Bluetooth library: `bluez-async`

- Linux only

- BLE GATT Central only

- Typesafe async wrapper around BlueZ D-Bus interface.

- Sent patches upstream to `dbus-rs` to improve code generation and support for complex types.

- Didn't announce it anywhere, but issues filed (and a PR) by two other users so far.

<!-- Talk about how btleplug main branch now uses bluez-async? -->

---

<!-- TODO: move this directly after bluetooth slide -->

# Concurrency

- Problem with single-threaded blocking Bluetooth library:
  ![](./single-threaded-blocking.svg)
  <!-- TODO: add lines for publishing the readings -->

---

# Concurrency

- Switch to async library:
  ![](./single-threaded-async.svg)
  <!-- TODO: add lines for publishing the readings -->
- But you all know javascript, so I don't have to tell you this.
<!-- FIXME: maybe they don't? -->

---

# Concurrency

- NOT SO FAST!
  ![](./single-threaded-mutex.svg)
- What if all of your sensors live in a big `Arc<Mutex<SensorState>>`?

---

# Concurrency

- Hold the Mutex for as little time as possible.
  ![](./single-threaded-mutex-final.svg)
- Much better.

---

# Concurrency (tools that we use)

- `Arc<Mutex<ALL THE THINGS>`

  - Fine as long as you're careful.

  - Only hold the mutex when you _need_ it.

- `Stream<Item = Event>`

  - Kinda fine.

  - Just the async version of Iter, but with less syntax support.

  - Not something that David uses much in web-land.

- Unbounded Channels

  - Fine if you know it's not going to back up.

---

# Observations about the project

- Separating things into modules (and crates) worked well:

  - App (`mijia-homie`) -> Sensor (`mijia`) -> Bluetooth (`bluez-async`) -> `bluez-generated` -> D-Bus.

  - App (`mijia-homie`) -> Homie (`homie-device`) -> MQTT.

  - MQTT -> Homie (`homie-controller`) -> `homie-influx` -> InfluxDB

- Deployment

  - Built with Github Actions and `cross`, packaged with `cargo-deb`, hosted on Bintray.
    <!-- FIXME: except it's not, is it, because bintray is dead? -->
    <!-- cross compiling to ARM is a pain if you need c libs, but cross makes it okay -->
    <!-- cross compiling to ARM v6 even more of is a pain, as Will can testify, but we got there in the end -->

  - Everything is supervised by systemd.

  - Test coverage is a bit thin.

- Desktop Linux tech stack (D-Bus, BlueZ) is not great.
- Raspberry Pi only supports 10 connected BLE devices (10 << 100).
  - My laptop only supports 7.
  - We added a USB Bluetooth adapter, and got a second Raspberry Pi.

---

<!-- TODO: related developments:

* bluez-async/btleplug/etc.
  * ???
* cloudbbq-homie
  * Architecture diagram
  * Graph of some meat
* mi plant sensor
  * Architecture diagram
  * ->mqtt written in python
  * graphs

 -->

---

# Links

- GitHub: https://github.com/alsuren/mijia-homie

- Homie helper library https://crates.io/crates/homie-device

- Bluetooth library https://crates.io/crates/bluez-async

# Questions

- ?

--

# Questions from me

- Does anyone have ideas about which graphs we should draw?
- What Bluetooth devices should we play with next?
