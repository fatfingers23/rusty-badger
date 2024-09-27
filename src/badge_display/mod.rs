pub mod display_image;

use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU8},
};
use cortex_m::interrupt::CriticalSection;
use defmt::*;
use display_image::get_current_image;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_rp::gpio;
use embassy_rp::gpio::Input;
use embassy_sync::{
    blocking_mutex::{self, raw::CriticalSectionRawMutex},
    mutex,
};
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
use uc8151::LUT;
use uc8151::WIDTH;
use uc8151::{asynch::Uc8151, UpdateRegion};
use {defmt_rtt as _, panic_probe as _};

use crate::{env::env_value, helpers::easy_format, Spi0Bus};

//Display state
pub static CURRENT_IMAGE: AtomicU8 = AtomicU8::new(0);
pub static CHANGE_IMAGE: AtomicBool = AtomicBool::new(true);
pub static WIFI_COUNT: AtomicU32 = AtomicU32::new(0);

pub static RTC_TIME_STRING: blocking_mutex::Mutex<CriticalSectionRawMutex, RefCell<String<8>>> =
    blocking_mutex::Mutex::new(RefCell::new(String::<8>::new()));

pub static HOUR: AtomicU8 = AtomicU8::new(10);
pub static MINUTE: AtomicU8 = AtomicU8::new(57);
pub static SECOND: AtomicU8 = AtomicU8::new(0);

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

    // Initialise display with speed
    let _ = display.setup(LUT::Medium).await;

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
    let display_text = easy_format::<29>(format_args!(
        "{}\n{}",
        env_value("NAME"),
        env_value("DETAILS")
    ));

    // \nWritten in rust\nRunning on a pico w";
    let name_and_detail_box = TextBox::with_textbox_style(
        &display_text,
        name_and_detail_bounds,
        character_style,
        textbox_style,
    );

    // Draw the text box.
    name_and_detail_box.draw(&mut display).unwrap();

    // let _ = display.update().await;

    //Each cycle is half a second
    let cycle: Duration = Duration::from_millis(500);

    let mut first_run = true;
    //New start every 120 cycles or 60 seconds
    let cycles_to_clear_at: i32 = 120;
    let mut cycles_since_last_clear = 0;

    loop {
        //Timed based display events

        //Runs every second;
        if cycles_since_last_clear % 2 == 0 {
            // update_time_values_from_cycles();
        }

        //Updates the top bar
        //Runs every 30 cycles/15 seconds and first run
        if cycles_since_last_clear % 30 == 0 || first_run {
            let count = WIFI_COUNT.load(core::sync::atomic::Ordering::Relaxed);
            let count_text: String<16> = easy_format::<16>(format_args!("Count: {}", count));
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

            Text::new(count_text.as_str(), Point::new(8, 16), character_style)
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
            // let _ = display.clear(Rgb565::WHITE.into());
            // let _ = display.update().await;
            WIFI_COUNT.store(count + 1, core::sync::atomic::Ordering::Relaxed);
        }

        //Runs every 120 cycles/60 seconds and first run
        if cycles_since_last_clear == 0 {
            let mut time_text: String<8> = String::<8>::new();

            let time_box_rectangle_location = Point::new(0, 96);
            RTC_TIME_STRING.lock(|x| {
                time_text.push_str(x.borrow().as_str()).unwrap();
            });

            //The bounds of the box for time and refresh area
            let time_bounds = Rectangle::new(time_box_rectangle_location, Size::new(88, 24));
            time_bounds
                .into_styled(
                    PrimitiveStyleBuilder::default()
                        .stroke_color(BinaryColor::Off)
                        .fill_color(BinaryColor::On)
                        .stroke_width(1)
                        .build(),
                )
                .draw(&mut display)
                .unwrap();

            //Adding a y offset to the box location to fit inside the box
            Text::new(
                time_text.as_str(),
                (
                    time_box_rectangle_location.x + 8,
                    time_box_rectangle_location.y + 16,
                )
                    .into(),
                character_style,
            )
            .draw(&mut display)
            .unwrap();

            let result = display
                .partial_update(time_bounds.try_into().unwrap())
                .await;
            match result {
                Ok(_) => {}
                Err(_) => {
                    info!("Error updating display");
                }
            }
        }

        //Manually triggered display events

        if CHANGE_IMAGE.load(core::sync::atomic::Ordering::Relaxed) {
            let current_image = get_current_image();
            let tga: Bmp<BinaryColor> = Bmp::from_slice(&current_image.image()).unwrap();
            let image = Image::new(&tga, current_image.image_location());
            //clear image location by writing a white rectangle over previous image location
            let image_bounds = Size::new(160, 140);
            let clear_rectangle = Rectangle::new(
                current_image.previous().image_location(),
                Size::new(157, 101),
            );
            clear_rectangle
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(&mut display)
                .unwrap();

            let _ = image.draw(&mut display);
            //TODO need to look up the reginal area display
            //             let update_region = UpdateRegion::new(192, 32, 160, 144).unwrap();
            // let result = display.partial_update(update_region).await;
            let _ = display.update().await;
            CHANGE_IMAGE.store(false, core::sync::atomic::Ordering::Relaxed);
        }

        cycles_since_last_clear += 1;
        if cycles_since_last_clear >= cycles_to_clear_at {
            cycles_since_last_clear = 0;
        }
        first_run = false;
        // info!("Display Cycle: {}", cycles_since_last_clear);
        Timer::after(cycle).await;
    }
}
