# Monitoring Temperature with Too Many Bluetooth Thermometers

Binary Solo 2020/10/16

Red Badger

---

# Outline

- Inception

- System Overview

- Bluetooth Libraries

- State Management

- Future Work

- Spin-off crates

---

# Introduction

- Housemate has a bunch of ESP32 sensors like this one

![](./inception-yun_hat_04.jpg)

---

# Introduction

- Wouldn't it be nice to have a hundred of these?

- Just imagine what you could do.

- What's the cheapest way to do this?

---

# Introduction

- Let's start with 20.

![](./inception-order.png)

---

# Introduction

- Obligatory Screenshot

![](./grafana-temperature.png)

---

# Introduction

- See also: fridge.

![](./grafana-fridge.png)

---

# System Overview

<!--
- Sensors
- Raspberry Pi
- Mosquitto (MQTT broker)
- openHAB
- InfluxDB
- Grafana
-->

![](./system-overview.svg)

---

# Bluetooth Libraries

- `blurz`

  - Started with this.
  - Blocking `device.connect()` calls.
  - Not multithreadded (because of how it uses D-Bus).
  - Unmaintained (for 2 years)

- `btleplug`

  - Mostly Async.
  - Talks directly to bluetooth stack over a socket.
  - Tried switching to this but gave up.

- `dbus-rs`
  - Async or Blocking (depending on which interface you use)
  - Generates code from introspection on the Raspberry Pi
  - Single-threaded in places (but that's okay).

---

# Concurrency

- Problem with single-threaded blocking bluetooth library:
  ![](./single-threaded-blocking.svg)

---

# Concurrency

- `Arc<Mutex<ALL THE THINGS>` and/or use channels. Clone is your friend.
- Only hold the mutex when you _need_ it, to avoid blocking other threads.
- (bonus hack): private `std::Mutex<Arc<Devices>>` and `get_devices() -> Arc<Devices>` for a copy-on-write snapshot.

---

# Future Work

- https://github.com/alsuren/mijia-homie/projects/1?fullscreen=true

---

# Spin-off crates

- homie-device
- homie-controller - eventually replace openHab in the above picture.
