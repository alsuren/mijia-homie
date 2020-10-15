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

- Sensors
- Raspberry Pi
- Mosqutitto (MQTT broker)
- openHAB
- InfluxDB
- Grafana

---

# Bluetooth Libraries

- blurz - blocking connect() calls; !Send; unmaintained (diagram)
- btleplug - promising but needs privs; crashy
- bluez-generated - in-house (literally); uses dbus-rs + dbus-codegen-rust; async; !Send in places

---

# State Management

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
