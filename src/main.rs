#![no_std]
#![no_main]

use crate::http::http_get;
use crate::pcf85063a::Control;
use badge_display::display_image::DisplayImage;
use badge_display::{
    CHANGE_IMAGE, CURRENT_IMAGE, DISPLAY_CHANGED, FORCE_SCREEN_REFRESH, RECENT_WIFI_NETWORKS,
    RTC_TIME_STRING, RecentWifiNetworksVec, SCREEN_TO_SHOW, Screen, WIFI_COUNT, run_the_display,
};
use core::cell::RefCell;
use core::fmt::Write;
use cyw43::JoinOptions;
use cyw43_pio::{DEFAULT_CLOCK_DIVIDER, PioSpi};
use defmt::info;
use defmt::*;
use embassy_embedded_hal::shared_bus::blocking::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_rp::clocks::RoscRng;
use embassy_rp::flash::Async;
use embassy_rp::gpio::Input;
use embassy_rp::i2c::I2c;
use embassy_rp::peripherals::{DMA_CH0, I2C0, PIO0, SPI0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::rtc::{DateTime, DayOfWeek};
use embassy_rp::spi::Spi;
use embassy_rp::spi::{self};
use embassy_rp::watchdog::Watchdog;
use embassy_rp::{bind_interrupts, gpio, i2c};
use embassy_sync::blocking_mutex::NoopMutex;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use env::env_value;
use gpio::{Level, Output, Pull};
use heapless::{String, Vec};
use helpers::easy_format;
use pcf85063a::PCF85063;
use save::{Save, read_postcard_from_flash, save_postcard_to_flash};
use serde::Deserialize;
use static_cell::StaticCell;
use time::PrimitiveDateTime;
use {defmt_rtt as _, panic_probe as _};

mod badge_display;
mod env;
mod helpers;
mod http;
mod pcf85063a;
mod save;

#[cfg(feature = "temp_sensor")]
mod temp_sensor;

type Spi0Bus = Mutex<NoopRawMutex, Spi<'static, SPI0, spi::Async>>;
type I2c0Bus = NoopMutex<RefCell<I2c<'static, I2C0, i2c::Blocking>>>;

const BSSID_LEN: usize = 1_000;
const ADDR_OFFSET: u32 = 0x100000;
const SAVE_OFFSET: u32 = 0x00;

const FLASH_SIZE: usize = 2 * 1024 * 1024;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

async fn blink(pin: &mut Output<'_>, n_times: usize) {
    for _ in 0..n_times {
        pin.set_high();
        Timer::after_millis(100).await;
        pin.set_low();
        Timer::after_millis(100).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut user_led = Output::new(p.PIN_22, Level::High);

    blink(&mut user_led, 1).await;

    //Wifi driver and cyw43 setup
    let fw = include_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        DEFAULT_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
    );
    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    spawner.must_spawn(cyw43_task(runner));

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let miso = p.PIN_16;
    let mosi = p.PIN_19;
    let clk = p.PIN_18;
    let dc = p.PIN_20;
    let cs = p.PIN_17;
    let busy = p.PIN_26;
    let reset = p.PIN_21;
    let power = p.PIN_10;

    let reset = Output::new(reset, Level::Low);
    let mut power = Output::new(power, Level::Low);
    power.set_high();

    let dc = Output::new(dc, Level::Low);
    let cs = Output::new(cs, Level::High);
    let busy = Input::new(busy, Pull::Up);

    let btn_up = Input::new(p.PIN_15, Pull::Down);
    let btn_down = Input::new(p.PIN_11, Pull::Down);
    let btn_a = Input::new(p.PIN_12, Pull::Down);
    let btn_b = Input::new(p.PIN_13, Pull::Down);
    let btn_c = Input::new(p.PIN_14, Pull::Down);
    let rtc_alarm = Input::new(p.PIN_8, Pull::Down);
    let mut watchdog = Watchdog::new(p.WATCHDOG);

    //Setup i2c bus
    let config = embassy_rp::i2c::Config::default();
    let i2c = i2c::I2c::new_blocking(p.I2C0, p.PIN_5, p.PIN_4, config);
    static I2C_BUS: StaticCell<I2c0Bus> = StaticCell::new();
    let i2c_bus = NoopMutex::new(RefCell::new(i2c));
    let i2c_bus = I2C_BUS.init(i2c_bus);

    let i2c_dev = I2cDevice::new(i2c_bus);
    let mut rtc_device = PCF85063::new(i2c_dev);

    if btn_a.is_high() {
        //Clears the alarm on start if A button is pressed (manual start)
        _ = rtc_device.disable_all_alarms();
        _ = rtc_device.clear_alarm_flag();
    }

    let spi = Spi::new(
        p.SPI0,
        clk,
        mosi,
        miso,
        p.DMA_CH1,
        p.DMA_CH2,
        spi::Config::default(),
    );

    //SPI Bus setup to run the e-ink display
    static SPI_BUS: StaticCell<Spi0Bus> = StaticCell::new();
    let spi_bus = SPI_BUS.init(Mutex::new(spi));

    blink(&mut user_led, 2).await;

    //wifi setup
    let config = embassy_net::Config::dhcpv4(Default::default());

    // Init network stack
    static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        config,
        RESOURCES.init(StackResources::new()),
        RoscRng.next_u64(),
    );

    //If the watch dog isn't fed, reboot to help with hang up
    watchdog.start(Duration::from_secs(8));

    spawner.must_spawn(net_task(runner));
    //Attempt to connect to wifi to get RTC time loop for 2 minutes
    let mut wifi_connection_attempts = 0;
    let mut connected_to_wifi = false;

    let wifi_ssid = env_value("WIFI_SSID");
    let wifi_password = env_value("WIFI_PASSWORD");
    while wifi_connection_attempts < 30 {
        watchdog.feed();
        match control
            .join(wifi_ssid, JoinOptions::new(wifi_password.as_bytes()))
            .await
        {
            Ok(_) => {
                connected_to_wifi = true;
                info!("join successful");

                blink(&mut user_led, 3).await;

                break;
            }
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
        Timer::after(Duration::from_secs(1)).await;
        wifi_connection_attempts += 1;
    }

    if connected_to_wifi {
        //Feed the dog if it makes it this far
        watchdog.feed();
        info!("waiting for DHCP...");
        while !stack.is_config_up() {
            Timer::after_millis(100).await;
        }
        info!("DHCP is now up!");

        info!("waiting for link up...");
        while !stack.is_link_up() {
            Timer::after_millis(500).await;
        }
        info!("Link is up!");

        info!("waiting for stack to be up...");
        stack.wait_config_up().await;
        info!("Stack is up!");

        //RTC Web request
        let mut rx_buffer = [0; 8192];
        let url = env_value("TIME_API");
        info!("connecting to {}", &url);

        // Feeds the dog again for one last time
        watchdog.feed();

        //If the call goes through set the rtc
        match http_get(&stack, url, &mut rx_buffer).await {
            Ok(bytes) => {
                match serde_json_core::de::from_slice::<TimeApiResponse>(bytes) {
                    Ok((output, _used)) => {
                        //Deadlines am i right?
                        rtc_device
                            .set_datetime(&output.into())
                            .expect("TODO: panic message");

                        blink(&mut user_led, 4).await;
                    }
                    Err(_e) => {
                        error!("Failed to parse response body");
                        // return; // handle the error

                        blink(&mut user_led, 1).await;
                    }
                }
            }
            Err(e) => {
                error!("Failed to make HTTP request: {:?}", e);
                // return; // handle the error
            }
        };
        //leave the wifi no longer needed
        let _ = control.leave().await;
    }

    //Set up saving
    let mut flash = embassy_rp::flash::Flash::<_, Async, FLASH_SIZE>::new(p.FLASH, p.DMA_CH3);
    let mut save =
        read_postcard_from_flash(ADDR_OFFSET, &mut flash, SAVE_OFFSET).unwrap_or_else(|err| {
            error!("Error getting the save from the flash: {:?}", err);
            Save::new()
        });
    WIFI_COUNT.store(save.wifi_counted, core::sync::atomic::Ordering::Relaxed);

    //Task spawning
    #[cfg(feature = "temp_sensor")]
    spawner.must_spawn(temp_sensor::run_the_temp_sensor(i2c_bus));

    spawner.must_spawn(run_the_display(spi_bus, cs, dc, busy, reset));

    //Input loop
    let cycle = Duration::from_millis(100);
    let mut current_cycle = 0;
    let mut time_to_scan = true;
    //5 minutes(ish) idk it's late and my math is so bad rn
    let reset_cycle = 3_000;

    //RTC alarm stuff
    let mut go_to_sleep = false;
    let mut reset_cycles_till_sleep = 0;
    //Like 15ish mins??
    let sleep_after_cycles = 4;

    if rtc_alarm.is_high() {
        //sleep happened
        go_to_sleep = true;
        info!("Alarm went off");
        _ = rtc_device.disable_all_alarms();
        _ = rtc_device.clear_alarm_flag();
    } else {
        info!("Alarm was clear")
    }

    loop {
        //Keep feeding the dog
        watchdog.feed();

        //Change Image Button
        if btn_c.is_high() {
            info!("Button C pressed");
            reset_cycles_till_sleep = 0;
            let current_image = CURRENT_IMAGE.load(core::sync::atomic::Ordering::Relaxed);
            let new_image = DisplayImage::from_u8(current_image).unwrap().next();
            CURRENT_IMAGE.store(new_image.as_u8(), core::sync::atomic::Ordering::Relaxed);
            CHANGE_IMAGE.store(true, core::sync::atomic::Ordering::Relaxed);
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        if btn_a.is_high() {
            println!("{:?}", current_cycle);
            info!("Button A pressed");
            reset_cycles_till_sleep = 0;
            user_led.toggle();
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        if btn_down.is_high() {
            info!("Button Down pressed");
            reset_cycles_till_sleep = 0;
            SCREEN_TO_SHOW.lock(|screen| {
                screen.replace(Screen::WifiList);
            });
            DISPLAY_CHANGED.store(true, core::sync::atomic::Ordering::Relaxed);
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        if btn_up.is_high() {
            info!("Button Up pressed");
            reset_cycles_till_sleep = 0;
            SCREEN_TO_SHOW.lock(|screen| {
                screen.replace(Screen::Badge);
            });
            DISPLAY_CHANGED.store(true, core::sync::atomic::Ordering::Relaxed);
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        if btn_b.is_high() {
            info!("Button B pressed");
            reset_cycles_till_sleep = 0;
            SCREEN_TO_SHOW.lock(|screen| {
                if *screen.borrow() == Screen::Badge {
                    //IF on badge screen and b pressed reset wifi count
                    save.wifi_counted = 0;
                    save.bssid.clear();
                    WIFI_COUNT.store(0, core::sync::atomic::Ordering::Relaxed);
                    current_cycle = 0;
                }
            });

            let mut recent_networks = RecentWifiNetworksVec::new();
            let mut scanner = control.scan(Default::default()).await;

            while let Some(bss) = scanner.next().await {
                process_bssid(bss.bssid, &mut save.wifi_counted, &mut save.bssid);
                if recent_networks.len() < 8 {
                    let possible_ssid = core::str::from_utf8(&bss.ssid);
                    match possible_ssid {
                        Ok(ssid) => {
                            let removed_zeros = ssid.trim_end_matches(char::from(0));
                            let ssid_string: String<32> =
                                easy_format::<32>(format_args!("{}", removed_zeros));

                            if recent_networks.contains(&ssid_string) {
                                continue;
                            }
                            if ssid_string != "" {
                                let _ = recent_networks.push(ssid_string);
                            }
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
            }
            RECENT_WIFI_NETWORKS.lock(|recent_networks_vec| {
                recent_networks_vec.replace(recent_networks);
            });

            FORCE_SCREEN_REFRESH.store(true, core::sync::atomic::Ordering::Relaxed);
            Timer::after(Duration::from_millis(500)).await;

            continue;
        }

        match rtc_device.get_datetime() {
            Ok(now) => set_display_time(now),
            Err(_err) => {
                error!("Error getting time");
                RTC_TIME_STRING.lock(|rtc_time_string| {
                    rtc_time_string.borrow_mut().clear();
                    rtc_time_string.borrow_mut().push_str("Error").unwrap();
                });
            }
        };

        if time_to_scan {
            info!("Scanning for wifi networks");
            reset_cycles_till_sleep += 1;
            time_to_scan = false;
            let mut scanner = control.scan(Default::default()).await;
            while let Some(bss) = scanner.next().await {
                process_bssid(bss.bssid, &mut save.wifi_counted, &mut save.bssid);
            }
            WIFI_COUNT.store(save.wifi_counted, core::sync::atomic::Ordering::Relaxed);
            save_postcard_to_flash(ADDR_OFFSET, &mut flash, SAVE_OFFSET, &save).unwrap();
            info!("wifi_counted: {}", save.wifi_counted);
        }
        if current_cycle >= reset_cycle {
            current_cycle = 0;
            time_to_scan = true;
        }

        if reset_cycles_till_sleep >= sleep_after_cycles {
            info!("Going to sleep");
            reset_cycles_till_sleep = 0;
            go_to_sleep = true;
        }

        if go_to_sleep {
            info!("going to sleep");
            Timer::after(Duration::from_secs(25)).await;
            //Set the rtc and sleep for 15 minutes
            //goes to sleep for 15 mins
            _ = rtc_device.disable_all_alarms();
            _ = rtc_device.clear_alarm_flag();
            _ = rtc_device.set_alarm_minutes(15);
            _ = rtc_device.control_alarm_minutes(Control::On);
            _ = rtc_device.control_alarm_interrupt(Control::On);
            power.set_low();
        }

        current_cycle += 1;
        Timer::after(cycle).await;
    }
}

fn set_display_time(time: PrimitiveDateTime) {
    let mut am = true;
    let twelve_hour = if time.hour() == 0 {
        12
    } else if time.hour() == 12 {
        am = false;
        12
    } else if time.hour() > 12 {
        am = false;
        time.hour() - 12
    } else {
        time.hour()
    };

    let am_pm = if am { "AM" } else { "PM" };

    let formatted_time = easy_format::<8>(format_args!(
        "{:02}:{:02} {}",
        twelve_hour,
        time.minute(),
        am_pm
    ));

    RTC_TIME_STRING.lock(|rtc_time_string| {
        rtc_time_string.borrow_mut().clear();
        rtc_time_string
            .borrow_mut()
            .push_str(formatted_time.as_str())
            .unwrap();
    });
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[derive(Deserialize)]
struct TimeApiResponse<'a> {
    datetime: &'a str,
    day_of_week: u8,
}

impl<'a> From<TimeApiResponse<'a>> for DateTime {
    fn from(response: TimeApiResponse) -> Self {
        info!("Datetime: {:?}", response.datetime);
        //split at T
        let datetime = response.datetime.split('T').collect::<Vec<&str, 2>>();
        //split at -
        let date = datetime[0].split('-').collect::<Vec<&str, 3>>();
        let year = date[0].parse::<u16>().unwrap();
        let month = date[1].parse::<u8>().unwrap();
        let day = date[2].parse::<u8>().unwrap();
        //split at :
        let time = datetime[1].split(':').collect::<Vec<&str, 4>>();
        let hour = time[0].parse::<u8>().unwrap();
        let minute = time[1].parse::<u8>().unwrap();
        //split at .
        let second_split = time[2].split('.').collect::<Vec<&str, 2>>();
        let second = second_split[0].parse::<f64>().unwrap();
        let rtc_time = DateTime {
            year: year,
            month: month,
            day: day,
            day_of_week: match response.day_of_week {
                0 => DayOfWeek::Sunday,
                1 => DayOfWeek::Monday,
                2 => DayOfWeek::Tuesday,
                3 => DayOfWeek::Wednesday,
                4 => DayOfWeek::Thursday,
                5 => DayOfWeek::Friday,
                6 => DayOfWeek::Saturday,
                _ => DayOfWeek::Sunday,
            },
            hour,
            minute,
            second: second as u8,
        };

        rtc_time
    }
}

fn process_bssid(bssid: [u8; 6], wifi_counted: &mut u32, bssids: &mut Vec<String<17>, BSSID_LEN>) {
    let bssid_str = format_bssid(bssid);
    if !bssids.contains(&bssid_str) {
        *wifi_counted += 1;
        WIFI_COUNT.store(*wifi_counted, core::sync::atomic::Ordering::Relaxed);
        // info!("bssid: {:x}", bssid_str);
        let result = bssids.push(bssid_str);
        if result.is_err() {
            info!("bssid list full");
            bssids.clear();
        }
    }
}

fn format_bssid(bssid: [u8; 6]) -> String<17> {
    let mut s = String::new();
    for (i, byte) in bssid.iter().enumerate() {
        if i != 0 {
            let _ = s.write_char(':');
        }
        core::fmt::write(&mut s, format_args!("{:02x}", byte)).unwrap();
    }
    s
}
