//! This example test the RP Pico W on board LED.
//!
//! It does not work with the RP Pico board.

#![no_std]
#![no_main]
use badge_display::display_image::DisplayImage;
use badge_display::{run_the_display, CHANGE_IMAGE, CURRENT_IMAGE, RTC_TIME_STRING};
use defmt::info;
use embassy_executor::Spawner;
use embassy_rp::gpio;
use embassy_rp::gpio::Input;
use embassy_rp::peripherals::SPI0;
use embassy_rp::rtc::{DateTime, DayOfWeek};
use embassy_rp::spi::Spi;
use embassy_rp::spi::{self};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Duration, Timer};
use embedded_graphics::{
    image::Image,
    mono_font::{ascii::*, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_text::{
    alignment::HorizontalAlignment,
    style::{HeightMode, TextBoxStyleBuilder},
    TextBox,
};
use gpio::{Level, Output, Pull};
use helpers::easy_format;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

mod badge_display;
mod cyw43_driver;
mod env;
mod helpers;

type Spi0Bus = Mutex<NoopRawMutex, Spi<'static, SPI0, spi::Async>>;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    // let (_net_device, mut control) = setup_cyw43(
    //     p.PIO0, p.PIN_23, p.PIN_24, p.PIN_25, p.PIN_29, p.DMA_CH0, spawner,
    // )
    // .await;

    // let input = gpio::Input::new(p.PIN_29, gpio::Pull::Up);

    let miso = p.PIN_16;
    let mosi = p.PIN_19;
    let clk = p.PIN_18;
    let dc = p.PIN_20;
    let cs = p.PIN_17;
    let busy = p.PIN_26;
    let reset = p.PIN_21;
    let power = p.PIN_10;

    let reset = Output::new(reset, Level::Low);
    let _power = Output::new(power, Level::Low);

    let dc = Output::new(dc, Level::Low);
    let cs = Output::new(cs, Level::High);
    let busy = Input::new(busy, Pull::Up);

    let _btn_up = Input::new(p.PIN_15, Pull::Down);
    let _btn_down = Input::new(p.PIN_11, Pull::Down);
    let _btn_a = Input::new(p.PIN_12, Pull::Down);
    let _btn_b = Input::new(p.PIN_13, Pull::Down);
    let btn_c = Input::new(p.PIN_14, Pull::Down);

    // let mut btn_c: Debouncer<'_> = Debouncer::new(Input::new(btn_c, Pull::Up), Duration::from_millis(20));

    let spi = Spi::new(
        p.SPI0,
        clk,
        mosi,
        miso,
        p.DMA_CH1,
        p.DMA_CH2,
        spi::Config::default(),
    );
    // let spi_bus: Mutex<NoopRawMutex, _> = Mutex::new(spi);
    static SPI_BUS: StaticCell<Spi0Bus> = StaticCell::new();
    let spi_bus = SPI_BUS.init(Mutex::new(spi));

    info!("led on!");
    // control.gpio_set(0, true).await;
    spawner.must_spawn(run_the_display(spi_bus, cs, dc, busy, reset));

    //rtc setup
    let mut rtc = embassy_rp::rtc::Rtc::new(p.RTC);
    if !rtc.is_running() {
        info!("Start RTC");
        let now = DateTime {
            year: 2000,
            month: 1,
            day: 1,
            day_of_week: DayOfWeek::Saturday,
            hour: 0,
            minute: 0,
            second: 0,
        };
        rtc.set_datetime(now).unwrap();
    }

    //Input loop
    loop {
        //Change Image Button
        if btn_c.is_high() {
            info!("Button C pressed");
            let current_image = CURRENT_IMAGE.load(core::sync::atomic::Ordering::Relaxed);
            let new_image = DisplayImage::from_u8(current_image).unwrap().next();
            CURRENT_IMAGE.store(new_image.as_u8(), core::sync::atomic::Ordering::Relaxed);
            CHANGE_IMAGE.store(true, core::sync::atomic::Ordering::Relaxed);
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        let now = rtc.now();
        match now {
            Ok(time) => set_display_time(time),
            Err(_) => {
                info!("Error getting time");
            }
        }

        Timer::after(Duration::from_millis(100)).await;
    }
}

fn set_display_time(time: DateTime) {
    let mut am = true;
    let twelve_hour = if time.hour > 12 {
        am = false;
        time.hour - 12
    } else if time.hour == 0 {
        12
    } else {
        time.hour
    };

    let am_pm = if am { "AM" } else { "PM" };

    let formatted_time = easy_format::<8>(format_args!(
        "{:02}:{:02} {}",
        twelve_hour, time.minute, am_pm
    ));

    RTC_TIME_STRING.lock(|rtc_time_string| {
        rtc_time_string.borrow_mut().clear();
        rtc_time_string
            .borrow_mut()
            .push_str(formatted_time.as_str())
            .unwrap();
    });
}
