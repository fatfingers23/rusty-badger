# Badger 2040 W written in rust

A rust controlled Badge for a conferences or anywhere else you want to be extra 

## Features
* Display some text to the left like name and job title
* Display a small bmp image, can alternate images by pressing the c button. This example has Ferris with a knife and a QR code that links to this repo
* Connects to a [Adafruit Sensirion SHTC3](https://www.adafruit.com/product/4636) via STEMMA QT / Qwiic to get real time temperature and humidity 
* If you set a wifi network in [.env](.env) the badge will set the pico's RTC and display the time one the display.
* Counts unique wifi bssid's it comes across and keeps those counts unique across reboots by writing to flash.


## Timings
The project is a mosh posh of things to get it ready for an event I am going to this weekend, so it is not always the best code or well thought out. Especially timings, I did not want to always refresh everything as fast as possible for battery and Eink constraints. 
* roughly every 5 mins it checks for new wifi networks
* roughly every 30 seconds it takes a new temp/humidity reading
* roughly every min it updates the time display, altho the RTC should keep pretty accurate timing
* roughly every 30 seconds it updates the top bar that holds wifi count as well as sensor data


## This project would not be possible without..
* [trvswgnr](https://github.com/trvswgnr) for their amazing ferris with a knife image. All i did was badly convert it to grayscale and scaled it down. 
* embassy framework and their great [examples](https://github.com/embassy-rs/embassy/tree/main/examples/rp). Exactly zero chance I would have any of this written without this directory.
* the [uc8151-rs](https://crates.io/crates/uc8151) crate. Would not be able to write to the e ink display without this great crate.
* And every other single crate found in [Cargo.toml](./Cargo.toml). None of it would be possible with out those packages and maintainers.