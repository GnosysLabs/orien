//! Pictures people upload: each is decoded, cut to size and saved again as
//! a plain JPEG, so whatever was hidden in the file, such as the place a
//! photo was taken, never reaches anyone else.

use anyhow::{Context, Result, bail};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageReader, Limits};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// The largest upload taken, in bytes.
pub const MAX_UPLOAD: usize = 8 << 20;
/// A profile picture's width and height, in pixels.
pub const PICTURE: u32 = 400;

fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    if bytes.len() > MAX_UPLOAD {
        bail!("that picture is too big; pick one under 8 MB");
    }
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().context("cannot read that picture")?;
    let mut limits = Limits::default();
    // Refuses pictures that claim huge sizes to use up the server's memory.
    limits.max_image_width = Some(12_000);
    limits.max_image_height = Some(12_000);
    limits.max_alloc = Some(256 << 20);
    reader.limits(limits);
    reader.decode().map_err(|_| anyhow::anyhow!("that is not a picture this site can read; use a JPEG, PNG, WebP or GIF"))
}

fn jpeg(image: &DynamicImage) -> Vec<u8> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 85).encode_image(&image.to_rgb8()).expect("encoding to memory");
    out
}

/// A square profile picture, cut from the middle.
pub fn profile_picture(bytes: &[u8]) -> Result<Vec<u8>> {
    let image = decode(bytes)?;
    Ok(jpeg(&image.resize_to_fill(PICTURE, PICTURE, image::imageops::FilterType::Lanczos3)))
}

/// Where uploaded pictures are kept, each under a random name.
pub struct Folder {
    dir: PathBuf,
}

impl Folder {
    pub fn open(dir: &Path) -> Result<Folder> {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
        Ok(Folder { dir: dir.into() })
    }

    /// Saves a prepared picture and returns its name.
    pub fn save(&self, jpeg: &[u8]) -> Result<String> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
        let name = format!("{}.jpg", hex::encode(random));
        std::fs::write(self.dir.join(&name), jpeg)?;
        Ok(name)
    }

    /// A saved picture, if `name` is one of ours.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let ours = name.len() == 36 && name.ends_with(".jpg") && name[..32].bytes().all(|b| b.is_ascii_hexdigit());
        ours.then(|| std::fs::read(self.dir.join(name)).ok()).flatten()
    }

    pub fn remove(&self, name: &str) {
        if self.read(name).is_some() {
            let _ = std::fs::remove_file(self.dir.join(name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_comes_out_a_plain_square_jpeg() {
        let photo = DynamicImage::new_rgb8(1200, 800);
        let mut png = Vec::new();
        photo.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let out = profile_picture(&png).unwrap();
        let back = image::load_from_memory(&out).unwrap();
        assert_eq!((back.width(), back.height()), (PICTURE, PICTURE));
        assert_eq!(&out[..2], &[0xFF, 0xD8], "a JPEG");
        assert!(profile_picture(b"not a picture").is_err());
    }

    #[test]
    fn only_names_it_gave_out_can_be_read() {
        let dir = std::env::temp_dir().join(format!("orien-market-images-{}", std::process::id()));
        let folder = Folder::open(&dir).unwrap();
        let name = folder.save(b"jpeg bytes").unwrap();
        assert_eq!(folder.read(&name).as_deref(), Some(&b"jpeg bytes"[..]));
        assert_eq!(folder.read("../market.db"), None);
        assert_eq!(folder.read("x".repeat(32).as_str()), None);
        folder.remove(&name);
        assert_eq!(folder.read(&name), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
