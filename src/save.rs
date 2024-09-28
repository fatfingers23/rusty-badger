use crate::FLASH_SIZE;
use embassy_rp::flash::{Async, ERASE_SIZE};
use embassy_rp::peripherals::FLASH;
use heapless::{String, Vec};
use postcard::{from_bytes, to_slice};
use serde::{Deserialize, Serialize};
use {defmt_rtt as _, panic_probe as _};

const BSSID_LEN: usize = 1_000;

pub fn save_postcard_to_flash(
    base_offset: u32,
    flash: &mut embassy_rp::flash::Flash<'_, FLASH, Async, FLASH_SIZE>,
    offset: u32,
    data: &Save,
) -> Result<(), &'static str> {
    let mut buf = [0u8; ERASE_SIZE];

    let mut write_buf = [0u8; ERASE_SIZE];
    let written = to_slice(data, &mut write_buf).map_err(|_| "Serialization error")?;

    if written.len() > ERASE_SIZE {
        return Err("Data too large for flash sector");
    }

    flash
        .blocking_erase(
            base_offset + offset,
            base_offset + offset + ERASE_SIZE as u32,
        )
        .map_err(|_| "Erase error")?;

    buf[..written.len()].copy_from_slice(&written);

    flash
        .blocking_write(base_offset + offset, &buf)
        .map_err(|_| "Write error")?;

    Ok(())
}

pub fn read_postcard_from_flash(
    base_offset: u32,
    flash: &mut embassy_rp::flash::Flash<'_, FLASH, Async, FLASH_SIZE>,
    offset: u32,
) -> Result<Save, &'static str> {
    let mut buf = [0u8; ERASE_SIZE];

    flash
        .blocking_read(base_offset + offset, &mut buf)
        .map_err(|_| "Read error")?;

    let data = from_bytes::<Save>(&buf).map_err(|_| "Deserialization error")?;

    Ok(data)
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
pub struct Save {
    pub wifi_counted: u32,
    pub bssid: Vec<String<17>, BSSID_LEN>,
}
