use crate::{CapturedImage, Result, WinScreenError};
use arboard::{Clipboard, ImageData};
use image::{codecs::jpeg::JpegEncoder, ColorType, ImageEncoder};
use std::borrow::Cow;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub fn save_png(image: &CapturedImage, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    let file = File::create(path).map_err(|source| WinScreenError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let writer = BufWriter::new(file);

    image::codecs::png::PngEncoder::new(writer).write_image(
        &image.rgba,
        image.width,
        image.height,
        ColorType::Rgba8.into(),
    )?;

    Ok(())
}

pub fn save_jpeg(image: &CapturedImage, path: impl AsRef<Path>, quality: u8) -> Result<()> {
    let path = path.as_ref();
    let file = File::create(path).map_err(|source| WinScreenError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let writer = BufWriter::new(file);

    let mut rgb = Vec::with_capacity((image.width * image.height * 3) as usize);
    for px in image.rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }

    JpegEncoder::new_with_quality(writer, quality).write_image(
        &rgb,
        image.width,
        image.height,
        ColorType::Rgb8.into(),
    )?;

    Ok(())
}

pub fn load_image(path: impl AsRef<Path>) -> Result<CapturedImage> {
    let path = path.as_ref();
    let image = image::open(path).map_err(WinScreenError::Image)?.to_rgba8();
    CapturedImage::new(image.width(), image.height(), image.into_raw())
}

pub fn write_clipboard_image(image: &CapturedImage) -> Result<()> {
    let mut clipboard =
        Clipboard::new().map_err(|err| WinScreenError::Clipboard(err.to_string()))?;
    clipboard
        .set_image(ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: Cow::Borrowed(&image.rgba),
        })
        .map_err(|err| WinScreenError::Clipboard(err.to_string()))
}

pub fn read_clipboard_image() -> Result<CapturedImage> {
    let mut clipboard =
        Clipboard::new().map_err(|err| WinScreenError::Clipboard(err.to_string()))?;
    let image = clipboard
        .get_image()
        .map_err(|err| WinScreenError::Clipboard(err.to_string()))?;
    CapturedImage::new(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
}
