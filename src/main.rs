//! This example test the RP Pico W on board LED.
//!
//! It does not work with the RP Pico board.

#![no_std]
#![no_main]
use core::sync::atomic::AtomicU32;
use defmt::info;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio;
use embassy_rp::gpio::Input;
use embassy_rp::peripherals::SPI0;
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
use heapless::String;
use image_handler::{get_current_image, DisplayImage, CHANGE_IMAGE, CURRENT_IMAGE};
use static_cell::StaticCell;
use tinybmp::Bmp;
use uc8151::asynch::Uc8151;
use uc8151::LUT;
use uc8151::WIDTH;
use {defmt_rtt as _, panic_probe as _};

mod cyw43_driver;
mod image_handler;

type Spi0Bus = Mutex<NoopRawMutex, Spi<'static, SPI0, spi::Async>>;

static WIFI_COUNT: AtomicU32 = AtomicU32::new(0);

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
        Timer::after(Duration::from_millis(100)).await;
    }
}

#[embassy_executor::task]
async fn run_the_display(
    spi_bus: &'static Spi0Bus,
    cs: Output<'static>,
    dc: Output<'static>,
    busy: Input<'static>,
    reset: Output<'static>,
) {
    let spi_dev = SpiDevice::new(&spi_bus, cs);

    let mut display = Uc8151::new(spi_dev, dc, busy, reset, Delay);

    display.reset().await;

    // Initialise display. Using the default LUT speed setting
    let _ = display.setup(LUT::Fast).await;

    // Note we're setting the Text color to `Off`. The driver is set up to treat Off as Black so that BMPs work as expected.
    let character_style = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::Off);
    let textbox_style = TextBoxStyleBuilder::new()
        .height_mode(HeightMode::FitToText)
        .alignment(HorizontalAlignment::Left)
        .paragraph_spacing(6)
        .build();

    // Bounding box for our text. Fill it with the opposite color so we can read the text.
    let name_and_detail_bounds = Rectangle::new(Point::new(0, 40), Size::new(WIDTH - 75, 0));
    name_and_detail_bounds
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .unwrap();

    // Create the text box and apply styling options.

    let text = "Bailey Townsend\nSoftware Dev";
    // \nWritten in rust\nRunning on a pico w";
    let name_and_detail_box =
        TextBox::with_textbox_style(text, name_and_detail_bounds, character_style, textbox_style);

    // Draw the text box.
    name_and_detail_box.draw(&mut display).unwrap();

    let _ = display.update().await;

    let delay: Duration = Duration::from_secs(30);
    let mut text: String<16> = String::<16>::new();

    loop {
        let count = WIFI_COUNT.load(core::sync::atomic::Ordering::Relaxed);
        // let _ = core::fmt::write(&mut text, format_args!("Count: {}", count));
        // let count_bounds = Rectangle::new(Point::new(0, 0), Size::new(WIDTH, 24));
        // count_bounds
        //     .into_styled(
        //         PrimitiveStyleBuilder::default()
        //             .stroke_color(BinaryColor::Off)
        //             .fill_color(BinaryColor::On)
        //             .stroke_width(1)
        //             .build(),
        //     )
        //     .draw(&mut display)
        //     .unwrap();

        // Text::new(text.as_str(), Point::new(8, 16), character_style)
        //     .draw(&mut display)
        //     .unwrap();

        // // // Draw the text box.
        // let result = display
        //     .partial_update(count_bounds.try_into().unwrap())
        //     .await;
        // match result {
        //     Ok(_) => {}
        //     Err(_) => {
        //         info!("Error updating display");
        //     }
        // }
        // text.clear();
        // let _ = display.clear(Rgb565::WHITE.into());
        // let _ = display.update().await;
        WIFI_COUNT.store(count + 1, core::sync::atomic::Ordering::Relaxed);

        if CHANGE_IMAGE.load(core::sync::atomic::Ordering::Relaxed) {
            let current_image = get_current_image();
            let tga: Bmp<BinaryColor> = Bmp::from_slice(&current_image.image()).unwrap();
            let image = Image::new(&tga, current_image.image_location());
            //clear image location by writing a white rectangle over previous image location
            let clear_bounds = Rectangle::new(
                current_image.previous().image_location(),
                Size::new(157, 91),
            );
            clear_bounds
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(&mut display)
                .unwrap();

            let _ = image.draw(&mut display);
            let _ = display.update().await;
            CHANGE_IMAGE.store(false, core::sync::atomic::Ordering::Relaxed);
        }

        Timer::after(Duration::from_millis(500)).await;
    }
}
