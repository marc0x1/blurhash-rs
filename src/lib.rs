//! A pure Rust implementation of [woltapp/blurhash][1].
//!
//! ### Encoding
//!
//! ```
//! use blurhash::encode;
//! use image::{GenericImageView, EncodableLayout};
//!
//! let img = image::open("data/octocat.png").unwrap();
//! let (width, height) = img.dimensions();
//! let blurhash = encode(4, 3, width, height, img.to_rgba8().as_bytes()).unwrap();
//!
//! assert_eq!(blurhash, "LNAdAqj[00aymkj[TKay9}ay-Sj[");
//! ```
//!
//! ### Decoding
//!
//! ```no_run
//! use blurhash::decode;
//!
//! let pixels = decode("LBAdAqof00WCqZj[PDay0.WB}pof", 50, 50, 1.0);
//! ```
//! [1]: https://github.com/woltapp/blurhash
mod ac;
mod base83;
mod dc;
mod error;
mod util;

pub use error::Error;

use std::f32::consts::PI;
use util::{linear_to_srgb, srgb_to_linear};

/// Calculates the blurhash for an image using the given x and y component counts.
pub fn encode(
    components_x: u32,
    components_y: u32,
    width: u32,
    height: u32,
    rgba_image: &[u8],
) -> Result<String, Error> {
    if !(1..=9).contains(&components_x) || !(1..=9).contains(&components_y) {
        return Err(Error::ComponentsOutOfRange);
    }

    let mut factors: Vec<[f32; 3]> = Vec::new();

    for y in 0..components_y {
        for x in 0..components_x {
            let factor = multiply_basis_function(x, y, width, height, rgba_image);
            factors.push(factor);
        }
    }

    let dc = factors[0];
    let ac = &factors[1..];

    let mut blurhash = String::new();

    let size_flag = (components_x - 1) + (components_y - 1) * 9;
    blurhash.push_str(&base83::encode(size_flag, 1));

    let maximum_value: f32;
    if !ac.is_empty() {
        let mut actualmaximum_value = 0.0;
        for i in 0..components_y * components_x - 1 {
            actualmaximum_value = f32::max(f32::abs(ac[i as usize][0]), actualmaximum_value);
            actualmaximum_value = f32::max(f32::abs(ac[i as usize][1]), actualmaximum_value);
            actualmaximum_value = f32::max(f32::abs(ac[i as usize][2]), actualmaximum_value);
        }

        let quantised_maximum_value = f32::max(
            0.,
            f32::min(82., f32::floor(actualmaximum_value * 166. - 0.5)),
        ) as u32;

        maximum_value = (quantised_maximum_value + 1) as f32 / 166.;
        blurhash.push_str(&base83::encode(quantised_maximum_value, 1));
    } else {
        maximum_value = 1.;
        blurhash.push_str(&base83::encode(0, 1));
    }

    blurhash.push_str(&base83::encode(dc::encode(dc), 4));

    for i in 0..components_y * components_x - 1 {
        blurhash.push_str(&base83::encode(
            ac::encode(ac[i as usize], maximum_value),
            2,
        ));
    }

    Ok(blurhash)
}

fn multiply_basis_function(
    component_x: u32,
    component_y: u32,
    width: u32,
    height: u32,
    rgb: &[u8],
) -> [f32; 3] {
    let mut r = 0.;
    let mut g = 0.;
    let mut b = 0.;
    let normalisation = match (component_x, component_y) {
        (0, 0) => 1.,
        _ => 2.,
    };

    let bytes_per_row = width * 4;

    for y in 0..height {
        for x in 0..width {
            let basis = f32::cos(PI * component_x as f32 * x as f32 / width as f32)
                * f32::cos(PI * component_y as f32 * y as f32 / height as f32);
            r += basis * srgb_to_linear(u32::from(rgb[(4 * x + y * bytes_per_row) as usize]));
            g += basis * srgb_to_linear(u32::from(rgb[(4 * x + 1 + y * bytes_per_row) as usize]));
            b += basis * srgb_to_linear(u32::from(rgb[(4 * x + 2 + y * bytes_per_row) as usize]));
        }
    }

    let scale = normalisation / (width * height) as f32;

    [r * scale, g * scale, b * scale]
}

/// Decodes the given blurhash to an image of the specified size into an existing buffer.
///
/// The punch parameter can be used to de- or increase the contrast of the
/// resulting image.
pub fn decode_into(
    pixels: &mut [u8],
    blurhash: &str,
    width: u32,
    height: u32,
    punch: f32,
) -> Result<(), Error> {
    let (num_x, num_y) = components(blurhash)?;

    assert_eq!(
        (width * height * 4) as usize,
        pixels.len(),
        "buffer length equals 4 * width * height"
    );

    let quantised_maximum_value = base83::decode(&blurhash[1..2])?;
    let maximum_value = (quantised_maximum_value + 1) as f32 / 166.;

    let mut colors = vec![[0.; 3]; num_x * num_y];

    for i in 0..colors.len() {
        if i == 0 {
            let value = base83::decode(&blurhash[2..6])?;
            colors[i] = dc::decode(value as u32);
        } else {
            let value = base83::decode(&blurhash[4 + i * 2..6 + i * 2])?;
            colors[i] = ac::decode(value as u32, maximum_value * punch);
        }
    }

    let bytes_per_row = width * 4;

    for y in 0..height {
        for x in 0..width {
            let mut pixel = [0.; 3];

            for j in 0..num_y {
                for i in 0..num_x {
                    let basis = f32::cos((PI * x as f32 * i as f32) / width as f32)
                        * f32::cos((PI * y as f32 * j as f32) / height as f32);
                    let color = &colors[i + j * num_x];

                    pixel[0] += color[0] * basis;
                    pixel[1] += color[1] * basis;
                    pixel[2] += color[2] * basis;
                }
            }

            let int_r = linear_to_srgb(pixel[0]);
            let int_g = linear_to_srgb(pixel[1]);
            let int_b = linear_to_srgb(pixel[2]);

            let pixels = &mut pixels[((4 * x + y * bytes_per_row) as usize)..][..4];

            pixels[0] = int_r as u8;
            pixels[1] = int_g as u8;
            pixels[2] = int_b as u8;
            pixels[3] = 255u8;
        }
    }
    Ok(())
}

/// Decodes the given blurhash to an image of the specified size.
///
/// The punch parameter can be used to de- or increase the contrast of the
/// resulting image.
pub fn decode(blurhash: &str, width: u32, height: u32, punch: f32) -> Result<Vec<u8>, Error> {
    let bytes_per_row = width * 4;
    let mut pixels = vec![0; (bytes_per_row * height) as usize];
    decode_into(&mut pixels, blurhash, width, height, punch).map(|()| pixels)
}

fn components(blurhash: &str) -> Result<(usize, usize), Error> {
    if blurhash.len() < 6 {
        return Err(Error::HashTooShort);
    }

    let size_flag = base83::decode(&blurhash[0..1])?;
    let num_y = (f32::floor(size_flag as f32 / 9.) + 1.) as usize;
    let num_x = (size_flag % 9) + 1;

    let expected = 4 + 2 * num_x * num_y;
    if blurhash.len() != expected {
        return Err(Error::LengthMismatch {
            expected,
            actual: blurhash.len(),
        });
    }

    Ok((num_x, num_y))
}

/// Calculates the blurhash for an [DynamicImage][image::DynamicImage] using the given x and y component counts.
#[cfg(feature = "image")]
pub fn encode_image(
    components_x: u32,
    components_y: u32,
    image: &image::RgbaImage,
) -> Result<String, Error> {
    use image::EncodableLayout;
    encode(
        components_x,
        components_y,
        image.width(),
        image.height(),
        image.as_bytes(),
    )
}

/// Calculates the blurhash for an [Pixbuf][gdk_pixbuf::Pixbuf] using the given x and y component counts.
///
/// Will panic if either the width or height of the image is negative.
#[cfg(feature = "gdk-pixbuf")]
pub fn encode_pixbuf(
    components_x: u32,
    components_y: u32,
    image: &gdk_pixbuf::Pixbuf,
) -> Result<String, Error> {
    use std::convert::TryInto;
    encode(
        components_x,
        components_y,
        image.width().try_into().expect("non-negative width"),
        image.height().try_into().expect("non-negative height"),
        &image.read_pixel_bytes(),
    )
}

/// Decodes the given blurhash to an image of the specified size.
///
/// The punch parameter can be used to de- or increase the contrast of the
/// resulting image.
#[cfg(feature = "image")]
pub fn decode_image(
    blurhash: &str,
    width: u32,
    height: u32,
    punch: f32,
) -> Result<image::RgbaImage, Error> {
    let bytes = decode(blurhash, width, height, punch)?;
    // Save to unwrap as `decode` (if successfull) always returns a buffer of size `4 * width * height`, which is exactly
    // the amount of bytes required to construct the `RgbaImage`.
    let buffer = image::RgbaImage::from_raw(width, height, bytes).expect("decoded image too small");
    Ok(buffer)
}

/// Decodes the given blurhash to an [Pixbuf][gdk_pixbuf::Pixbuf] of the specified size.
///
/// The punch parameter can be used to de- or increase the contrast of the
/// resulting image.
/// Will panic if the width or height does not fit in i32.
#[cfg(feature = "gdk-pixbuf")]
pub fn decode_pixbuf(
    blurhash: &str,
    width: u32,
    height: u32,
    punch: f32,
) -> Result<gdk_pixbuf::Pixbuf, Error> {
    use std::convert::TryInto;
    let bytes = decode(blurhash, width, height, punch)?;
    let width = width.try_into().expect("width fits in i32");
    let height = height.try_into().expect("height fits in i32");
    let buffer = gdk_pixbuf::Pixbuf::from_bytes(
        &gdk_pixbuf::glib::Bytes::from_owned(bytes),
        gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        width,
        height,
        4 * height,
    );
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{save_buffer, ColorType::Rgba8};
    use image::{EncodableLayout, GenericImageView};

    #[test]
    fn decode_blurhash() {
        let img = image::open("data/octocat.png").unwrap();
        let (width, height) = img.dimensions();

        let blurhash = encode(4, 3, width, height, img.to_rgba8().as_bytes()).unwrap();
        let img = decode(&blurhash, width, height, 1.0).unwrap();
        save_buffer("data/out.png", &img, width, height, Rgba8).unwrap();

        assert_eq!(img[0..5], [1, 1, 1, 255, 1]);
    }

    #[test]
    fn test_jelly_beans() {
        use image::{EncodableLayout, GenericImageView};

        let img = image::open("data/octocat.png").unwrap();
        let (width, height) = img.dimensions();
        let blurhash = encode(4, 3, width, height, img.to_rgba8().as_bytes()).unwrap();

        // assert_eq!(blurhash, "LFL4=pI8%foujXofRkWC.loyH?V{");
        assert_eq!(blurhash, "LNAdAqj[00aymkj[TKay9}ay-Sj[");
    }

    #[test]
    #[cfg(feature = "image")]
    fn test_jelly_beans_image() {
        let img = image::open("data/octocat.png").unwrap();

        let blurhash = encode_image(4, 3, &img.to_rgba8()).unwrap();

        assert_eq!(blurhash, "LNAdAqj[00aymkj[TKay9}ay-Sj[");
    }

    #[test]
    #[cfg(feature = "image")]
    fn decode_blurhash_image() {
        let img = image::open("data/octocat.png").unwrap();
        let (width, height) = img.dimensions();

        let blurhash = encode_image(4, 3, &img.to_rgba8()).unwrap();
        let img = decode_image(&blurhash, width, height, 1.0).unwrap();

        assert_eq!(img.as_bytes()[0..5], [1, 1, 1, 255, 1]);
    }

    #[test]
    #[cfg(feature = "gdk-pixbuf")]
    fn test_jelly_beans_pixbuf() {
        let img = gdk_pixbuf::Pixbuf::from_file("data/octocat.png").unwrap();

        let blurhash = encode_pixbuf(4, 3, &img).unwrap();

        assert_eq!(blurhash, "LNAdAqj[00aymkj[TKay9}ay-Sj[");
    }

    #[test]
    #[cfg(feature = "gdk-pixbuf")]
    fn decode_blurhash_pixbuf() {
        use std::convert::TryInto;
        let img = gdk_pixbuf::Pixbuf::from_file("data/octocat.png").unwrap();

        let blurhash = encode_pixbuf(4, 3, &img).unwrap();
        let img = decode_pixbuf(
            &blurhash,
            img.width().try_into().unwrap(),
            img.height().try_into().unwrap(),
            1.0,
        )
        .unwrap();

        assert_eq!(img.read_pixel_bytes()[0..5], [1, 1, 1, 255, 1]);
    }
}
