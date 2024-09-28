use defmt::*;
use embassy_rp::i2c::{I2c, SclPin, SdaPin};
use embassy_rp::peripherals::I2C0;
use embassy_rp::{i2c, Peripheral};
use embassy_time::Timer;
use shtcx::{self, PowerMode};

use crate::badge_display::{HUMIDITY, TEMP}; // Import the necessary items from shtcx

#[embassy_executor::task]
pub async fn run_the_temp_sensor(
    i2c0: I2C0,
    scl: impl Peripheral<P = impl SclPin<I2C0>> + 'static,
    sda: impl Peripheral<P = impl SdaPin<I2C0>> + 'static,
) {
    let i2c = I2c::new_blocking(i2c0, scl, sda, i2c::Config::default());

    let mut sht = shtcx::shtc3(i2c);
    let mut sht_delay = embassy_time::Delay; // Create a delay instance

    loop {
        let combined = sht.measure(PowerMode::NormalMode, &mut sht_delay).unwrap();
        let celsius = combined.temperature.as_degrees_celsius();
        let fahrenheit = (celsius * 9.0 / 5.0) + 32.0;
        info!(
            "Temperature: {}°F, Humidity: {}%",
            fahrenheit,
            combined.humidity.as_percent()
        );
        TEMP.store(fahrenheit as u8, core::sync::atomic::Ordering::Relaxed);
        HUMIDITY.store(
            combined.humidity.as_percent() as u8,
            core::sync::atomic::Ordering::Relaxed,
        );
        Timer::after_secs(30).await;
    }
}
