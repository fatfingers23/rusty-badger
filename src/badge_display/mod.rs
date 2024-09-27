pub mod display_image;

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8};
use defmt::*;
use display_image::get_current_image;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_rp::gpio;
use embassy_rp::gpio::Input;
use embassy_time::{Delay, Duration, Timer};
use embedded_graphics::{
    image::Image,
    mono_font::{ascii::*, MonoTextStyle},
    pixelcolor::{BinaryColor, Rgb565},
    prelude::*,
    primitives::{PrimitiveStyle, PrimitiveStyleBuilder, Rectangle},
    text::Text,
};
use embedded_text::{
    alignment::HorizontalAlignment,
    style::{HeightMode, TextBoxStyleBuilder},
    TextBox,
};
use gpio::Output;
use heapless::String;
use tinybmp::Bmp;
use uc8151::asynch::Uc8151;
use uc8151::LUT;
use uc8151::WIDTH;
use {defmt_rtt as _, panic_probe as _};

use crate::Spi0Bus;

//Display state
pub static CURRENT_IMAGE: AtomicU8 = AtomicU8::new(0);
pub static CHANGE_IMAGE: AtomicBool = AtomicBool::new(true);
pub static WIFI_COUNT: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
pub async fn run_the_display(
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

    let cycle: Duration = Duration::from_millis(500);
    let mut first_run = true;
    let mut text: String<16> = String::<16>::new();
    let cycles_to_skip = 30;
    let mut cycles_since_last_clear = 0;

    loop {
        if cycles_since_last_clear >= cycles_to_skip || first_run {
            let count = WIFI_COUNT.load(core::sync::atomic::Ordering::Relaxed);
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
            let _ = display.update().await;
            WIFI_COUNT.store(count + 1, core::sync::atomic::Ordering::Relaxed);
            cycles_since_last_clear = 0;
        }

        if CHANGE_IMAGE.load(core::sync::atomic::Ordering::Relaxed) {
            let current_image = get_current_image();
            let tga: Bmp<BinaryColor> = Bmp::from_slice(&current_image.image()).unwrap();
            let image = Image::new(&tga, current_image.image_location());
            //clear image location by writing a white rectangle over previous image location
            let clear_bounds = Rectangle::new(
                current_image.previous().image_location(),
                Size::new(157, 101),
            );
            clear_bounds
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(&mut display)
                .unwrap();

            let _ = image.draw(&mut display);
            let _ = display.update().await;
            CHANGE_IMAGE.store(false, core::sync::atomic::Ordering::Relaxed);
        }

        cycles_since_last_clear += 1;
        first_run = false;
        Timer::after(cycle).await;
    }
}
