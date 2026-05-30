// Copyright 2022-2022 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use objc2::{rc::Retained, AllocAnyThread};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSData, NSSize};

use crate::icon::{BadIcon, RgbaIcon};
use std::{borrow::Cow, io::Cursor};

#[derive(Debug, Clone)]
pub struct PlatformIcon(RgbaIcon);

impl PlatformIcon {
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, BadIcon> {
        Ok(PlatformIcon(RgbaIcon::from_rgba(rgba, width, height)?))
    }

    pub fn get_size(&self) -> (u32, u32) {
        (self.0.width, self.0.height)
    }

    fn normalized_rgba_for_png(&self) -> (u32, u32, Cow<'_, [u8]>) {
        let (width, height) = self.get_size();

        if width == 0 || height == 0 {
            return (1, 1, Cow::Owned(vec![0, 0, 0, 0]));
        }

        (width, height, Cow::Borrowed(&self.0.rgba))
    }

    pub fn to_png(&self) -> Vec<u8> {
        let (width, height, rgba) = self.normalized_rgba_for_png();
        let mut png = Vec::new();

        {
            let mut encoder = png::Encoder::new(Cursor::new(&mut png), width as _, height as _);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);

            let mut writer = encoder
                .write_header()
                .expect("normalized icon dimensions must be PNG-encodable");
            writer
                .write_image_data(rgba.as_ref())
                .expect("normalized icon RGBA length must match PNG dimensions");
        }

        png
    }

    pub fn to_nsimage(&self, fixed_height: Option<f64>) -> Retained<NSImage> {
        let (width, height, _) = self.normalized_rgba_for_png();
        let icon = self.to_png();

        let (icon_width, icon_height) = match fixed_height {
            Some(fixed_height) => {
                let icon_height: CGFloat = fixed_height as CGFloat;
                let icon_width: CGFloat = (width as CGFloat) / (height as CGFloat / icon_height);

                (icon_width, icon_height)
            }

            None => (width as CGFloat, height as CGFloat),
        };

        let nsdata = NSData::with_bytes(&icon);

        let nsimage = NSImage::initWithData(NSImage::alloc(), &nsdata)
            .expect("normalized PNG should create NSImage");
        let new_size = NSSize::new(icon_width, icon_height);
        unsafe { nsimage.setSize(new_size) };

        nsimage
    }
}

#[cfg(test)]
mod tests {
    use super::PlatformIcon;

    #[test]
    fn to_png_handles_zero_dimension_icon_without_panicking() {
        let icon = PlatformIcon::from_rgba(Vec::new(), 0, 0).expect("zero icon remains accepted");

        let png = icon.to_png();

        assert!(!png.is_empty());
    }

    #[test]
    fn to_nsimage_handles_zero_dimension_icon_without_panicking() {
        let icon = PlatformIcon::from_rgba(Vec::new(), 0, 0).expect("zero icon remains accepted");

        let _image = icon.to_nsimage(Some(18.0));
    }
}
