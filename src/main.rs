//! This example test the RP Pico W on board LED.
//!
//! It does not work with the RP Pico board.

#![no_std]
#![no_main]
use control_driver::setup_cyw43;
use defmt::info;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio;
use embassy_rp::gpio::Input;
use embassy_rp::spi::Spi;
use embassy_rp::spi::{self};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Duration, Timer};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::PrimitiveStyleBuilder;
use embedded_graphics::text::Text;
use embedded_graphics::{
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
use uc8151::asynch::Uc8151;
use uc8151::WIDTH;
use uc8151::{UpdateRegion, LUT};
use {defmt_rtt as _, panic_probe as _};
mod control_driver;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let (_net_device, mut control) = setup_cyw43(
        p.PIO0, p.PIN_23, p.PIN_24, p.PIN_25, p.PIN_29, p.DMA_CH0, spawner,
    )
    .await;

    let miso = p.PIN_16;
    let mosi = p.PIN_19;
    let clk = p.PIN_18;
    let dc = p.PIN_20;
    let cs = p.PIN_17;
    let busy = p.PIN_26;
    let reset = p.PIN_21;
    let power = p.PIN_10;

    let btn_up = p.PIN_15;
    let btn_down = p.PIN_11;
    let btn_a = p.PIN_12;
    let btn_b = p.PIN_13;
    let btn_c = p.PIN_14;

    let reset = Output::new(reset, Level::Low);
    let _power = Output::new(power, Level::Low);

    let dc = Output::new(dc, Level::Low);
    let cs = Output::new(cs, Level::High);
    let busy = Input::new(busy, Pull::Up);

    let mut _btn_up = Input::new(btn_up, Pull::Up);
    let mut _btn_down = Input::new(btn_down, Pull::Up);
    let mut _btn_a = Input::new(btn_a, Pull::Up);
    let mut _btn_b = Input::new(btn_b, Pull::Up);
    let mut _btn_c = Input::new(btn_c, Pull::Up);

    let spi = Spi::new(
        p.SPI0,
        clk,
        mosi,
        miso,
        p.DMA_CH1,
        p.DMA_CH2,
        spi::Config::default(),
    );
    let spi_bus: Mutex<NoopRawMutex, _> = Mutex::new(spi);
    let spi_dev = SpiDevice::new(&spi_bus, cs);
    let mut display = Uc8151::new(spi_dev, dc, busy, reset, Delay);

    display.reset().await;

    // Initialise display. Using the default LUT speed setting
    let _ = display.setup(LUT::Fast).await;

    // Note we're setting the Text color to `Off`. The driver is set up to treat Off as Black so that BMPs work as expected.
    let character_style = MonoTextStyle::new(&FONT_9X18_BOLD, BinaryColor::Off);
    let textbox_style = TextBoxStyleBuilder::new()
        .height_mode(HeightMode::FitToText)
        .alignment(HorizontalAlignment::Center)
        .paragraph_spacing(6)
        .build();

    // Bounding box for our text. Fill it with the opposite color so we can read the text.
    let bounds = Rectangle::new(Point::new(0, 40), Size::new(WIDTH - 157, 0));
    bounds
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut display)
        .unwrap();

    // Create the text box and apply styling options.
    let text = "Written In\nRust!!";
    let text_box = TextBox::with_textbox_style(text, bounds, character_style, textbox_style);

    // Draw the text box.
    text_box.draw(&mut display).unwrap();

    let _ = display.update().await;

    let delay: Duration = Duration::from_secs(1);
    let mut count = 0;
    let mut text: String<16> = String::<16>::new();

    loop {
        info!("led on!");
        control.gpio_set(0, true).await;
        Timer::after(delay).await;

        info!("led off!");
        control.gpio_set(0, false).await;

        Timer::after(delay).await;

        let _ = core::fmt::write(&mut text, format_args!("Count: {}", count));
        let count_bounds = Rectangle::new(Point::new(0, 0), Size::new(WIDTH, 24));
        count_bounds
            .into_styled(
                PrimitiveStyleBuilder::default()
                    .stroke_color(BinaryColor::Off)
                    .fill_color(BinaryColor::On)
                    .stroke_width(1)
                    .build(),
            )
            .draw(&mut display)
            .unwrap();

        Text::new(text.as_str(), Point::new(8, 16), character_style)
            .draw(&mut display)
            .unwrap();

        // // Draw the text box.
        let result = display
            .partial_update(count_bounds.try_into().unwrap())
            .await;
        match result {
            Ok(_) => {}
            Err(_) => {
                info!("Error updating display");
            }
        }
        text.clear();
        // let _ = display.clear(Rgb565::WHITE.into());
        // let _ = display.update().await;
        count += 1;
        // Timer::after(Duration::from_secs(10)).await;
    }
}
