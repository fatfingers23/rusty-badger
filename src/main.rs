//! This example test the RP Pico W on board LED.
//!
//! It does not work with the RP Pico board.

#![no_std]
#![no_main]
use badge_display::display_image::DisplayImage;
use badge_display::{run_the_display, CHANGE_IMAGE, CURRENT_IMAGE, RTC_TIME_STRING, WIFI_COUNT};
use core::fmt::Write;
use core::str::from_utf8;
use cyw43_driver::setup_cyw43;
use defmt::info;
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::{Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::flash::Async;
use embassy_rp::gpio;
use embassy_rp::gpio::Input;
use embassy_rp::peripherals::{DMA_CH0, PIO0, SPI0};
use embassy_rp::rtc::{DateTime, DayOfWeek};
use embassy_rp::spi::Spi;
use embassy_rp::spi::{self};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use embedded_hal_1::digital::OutputPin;
use env::env_value;
use gpio::{Level, Output, Pull};
use heapless::{String, Vec};
use helpers::easy_format;
use rand::RngCore;
use reqwless::client::{HttpClient, TlsConfig, TlsVerify};
use reqwless::request::Method;
use save::{read_postcard_from_flash, save_postcard_to_flash, Save};
use serde::Deserialize;
use static_cell::StaticCell;
use temp_sensor::run_the_temp_sensor;
use {defmt_rtt as _, panic_probe as _};

mod badge_display;
mod cyw43_driver;
mod env;
mod helpers;
mod save;
mod temp_sensor;

type Spi0Bus = Mutex<NoopRawMutex, Spi<'static, SPI0, spi::Async>>;

const BSSID_LEN: usize = 1_000;
const ADDR_OFFSET: u32 = 0x100000;
const SAVE_OFFSET: u32 = 0x00;

const FLASH_SIZE: usize = 2 * 1024 * 1024;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut user_led = Output::new(p.PIN_22, Level::High);
    user_led.set_high();

    let (net_device, mut control) = setup_cyw43(
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

    let reset = Output::new(reset, Level::Low);
    let _power = Output::new(power, Level::Low);

    let dc = Output::new(dc, Level::Low);
    let cs = Output::new(cs, Level::High);
    let busy = Input::new(busy, Pull::Up);

    let _btn_up = Input::new(p.PIN_15, Pull::Down);
    let _btn_down = Input::new(p.PIN_11, Pull::Down);
    let _btn_a = Input::new(p.PIN_12, Pull::Down);
    let btn_b = Input::new(p.PIN_13, Pull::Down);
    let btn_c = Input::new(p.PIN_14, Pull::Down);

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

    info!("led on!");
    // control.gpio_set(0, true).await;

    //wifi setup
    let mut rng = RoscRng;

    let config = embassy_net::Config::dhcpv4(Default::default());
    let seed = rng.next_u64();

    // Init network stack
    static STACK: StaticCell<Stack<cyw43::NetDriver<'static>>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
    let stack = &*STACK.init(Stack::new(
        net_device,
        config,
        RESOURCES.init(StackResources::<5>::new()),
        seed,
    ));
    //rtc setup
    let mut rtc = embassy_rp::rtc::Rtc::new(p.RTC);

    spawner.must_spawn(net_task(stack));
    //Attempt to connect to wifi to get RTC time loop for 2 minutes
    let mut wifi_connection_attempts = 0;
    let mut connected_to_wifi = false;

    let wifi_ssid = env_value("WIFI_SSID");
    let wifi_password = env_value("WIFI_PASSWORD");
    while wifi_connection_attempts < 30 {
        match control.join_wpa2(wifi_ssid, &wifi_password).await {
            Ok(_) => {
                connected_to_wifi = true;
                info!("join successful");
                break;
            }
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
        Timer::after(Duration::from_secs(1)).await;
        wifi_connection_attempts += 1;
    }

    let mut time_was_set = false;
    if connected_to_wifi {
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
        let mut tls_read_buffer = [0; 16640];
        let mut tls_write_buffer = [0; 16640];
        let client_state = TcpClientState::<1, 1024, 1024>::new();
        let tcp_client = TcpClient::new(stack, &client_state);
        let dns_client = DnsSocket::new(stack);
        let tls_config = TlsConfig::new(
            seed,
            &mut tls_read_buffer,
            &mut tls_write_buffer,
            TlsVerify::None,
        );

        let mut http_client = HttpClient::new_with_tls(&tcp_client, &dns_client, tls_config);
        let url = env_value("TIME_API");
        info!("connecting to {}", &url);

        let mut request = match http_client.request(Method::GET, &url).await {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to make HTTP request: {:?}", e);
                return; // handle the error
            }
        };

        let response = match request.send(&mut rx_buffer).await {
            Ok(resp) => resp,
            Err(_e) => {
                error!("Failed to send HTTP request");
                return; // handle the error;
            }
        };

        let body = match from_utf8(response.body().read_to_end().await.unwrap()) {
            Ok(b) => b,
            Err(_e) => {
                error!("Failed to read response body");
                return; // handle the error
            }
        };
        info!("Response body: {:?}", &body);

        let bytes = body.as_bytes();
        match serde_json_core::de::from_slice::<TimeApiResponse>(bytes) {
            Ok((output, _used)) => {
                //Deadlines am i right?
                info!("Datetime: {:?}", output.datetime);
                //split at T
                let datetime = output.datetime.split('T').collect::<Vec<&str, 2>>();
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
                    day_of_week: match output.day_of_week {
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
                rtc.set_datetime(rtc_time).unwrap();
                time_was_set = true;
            }
            Err(_e) => {
                error!("Failed to parse response body");
                return; // handle the error
            }
        }
    }

    //Set up saving
    let mut flash = embassy_rp::flash::Flash::<_, Async, FLASH_SIZE>::new(p.FLASH, p.DMA_CH3);
    let mut save: Save = read_postcard_from_flash(ADDR_OFFSET, &mut flash, SAVE_OFFSET).unwrap();
    WIFI_COUNT.store(save.wifi_counted, core::sync::atomic::Ordering::Relaxed);
    //Task spawning
    spawner.must_spawn(run_the_temp_sensor(p.I2C0, p.PIN_5, p.PIN_4));
    spawner.must_spawn(run_the_display(spi_bus, cs, dc, busy, reset));

    //Input loop
    let cycle = Duration::from_millis(100);
    let mut current_cycle = 0;
    //5 minutes
    let reset_cycle = 300_000;
    //Turn off led to signify that the badge is ready
    user_led.set_low();

    loop {
        //Change Image Button
        if btn_c.is_high() {
            info!("Button C pressed");
            let current_image = CURRENT_IMAGE.load(core::sync::atomic::Ordering::Relaxed);
            let new_image = DisplayImage::from_u8(current_image).unwrap().next();
            CURRENT_IMAGE.store(new_image.as_u8(), core::sync::atomic::Ordering::Relaxed);
            CHANGE_IMAGE.store(true, core::sync::atomic::Ordering::Relaxed);
            Timer::after(Duration::from_millis(500)).await;
            current_cycle += 500;
            continue;
        }

        if btn_b.is_high() {
            user_led.toggle();
            Timer::after(Duration::from_millis(500)).await;
            current_cycle += 500;
            continue;
        }

        if time_was_set {
            let now = rtc.now();
            match now {
                Ok(time) => set_display_time(time),
                Err(_) => {
                    info!("Error getting time");
                }
            }
        } else {
            RTC_TIME_STRING.lock(|rtc_time_string| {
                rtc_time_string.borrow_mut().clear();
                rtc_time_string.borrow_mut().push_str("No Wifi").unwrap();
            });
        }
        if current_cycle == 0 {
            let mut scanner = control.scan(Default::default()).await;
            while let Some(bss) = scanner.next().await {
                process_bssid(bss.bssid, &mut save.wifi_counted, &mut save.bssid);
                let ssid = core::str::from_utf8(&bss.ssid).unwrap();
                info!("ssid: {}", ssid);
            }
            save_postcard_to_flash(ADDR_OFFSET, &mut flash, SAVE_OFFSET, &save).unwrap();
            WIFI_COUNT.store(save.wifi_counted, core::sync::atomic::Ordering::Relaxed);
            info!("wifi_counted: {}", save.wifi_counted);
        }
        if current_cycle >= reset_cycle {
            current_cycle = 0;
        }
        current_cycle += 1;
        Timer::after(cycle).await;
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

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<cyw43::NetDriver<'static>>) -> ! {
    stack.run().await
}

#[derive(Deserialize)]
struct TimeApiResponse<'a> {
    datetime: &'a str,
    day_of_week: u8,
}

fn process_bssid(bssid: [u8; 6], wifi_counted: &mut u32, bssids: &mut Vec<String<17>, BSSID_LEN>) {
    let bssid_str = format_bssid(bssid);
    if !bssids.contains(&bssid_str) {
        *wifi_counted += 1;
        info!("bssid: {:x}", bssid_str);
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
