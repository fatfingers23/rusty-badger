use embedded_graphics::prelude::Point;

use super::CURRENT_IMAGE;

static NUMBER_OF_IMAGES: u8 = 2;
static FERRIS_IMG: &[u8; 15722] = include_bytes!("../../images/ferris_w_a_knife.bmp");
static REPO_IMG: &[u8; 11262] = include_bytes!("../../images/repo.bmp");

pub enum DisplayImage {
    Ferris = 0,
    Repo = 1,
}

pub fn get_current_image() -> DisplayImage {
    DisplayImage::from_u8(CURRENT_IMAGE.load(core::sync::atomic::Ordering::Relaxed)).unwrap()
}

impl DisplayImage {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Ferris),
            1 => Some(Self::Repo),
            _ => None,
        }
    }

    pub fn as_u8(&self) -> u8 {
        match self {
            Self::Ferris => 0,
            Self::Repo => 1,
        }
    }

    pub fn image(&self) -> &'static [u8] {
        match self {
            Self::Ferris => FERRIS_IMG,
            Self::Repo => REPO_IMG,
        }
    }

    pub fn next(&self) -> Self {
        let image_count = self.as_u8();
        let next_image = (image_count + 1) % NUMBER_OF_IMAGES;
        DisplayImage::from_u8(next_image).unwrap()
    }

    pub fn previous(&self) -> Self {
        let image_count = self.as_u8();
        if image_count == 0 {
            return DisplayImage::from_u8(NUMBER_OF_IMAGES - 1).unwrap();
        }
        let previous_image = (image_count - 1) % NUMBER_OF_IMAGES;
        DisplayImage::from_u8(previous_image).unwrap()
    }

    pub fn image_location(&self) -> Point {
        match self {
            Self::Ferris => Point::new(150, 26),
            Self::Repo => Point::new(190, 26),
        }
    }
}
