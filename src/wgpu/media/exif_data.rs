use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use exif::{Exif, Field, In, Reader, Tag, Value};

#[derive(Debug, Clone, Default)]
pub struct ExifData {
    pub make: Option<String>,
    pub model: Option<String>,
    pub datetime: Option<String>,
    pub exposure_time: Option<String>,
    pub f_number: Option<String>,
    pub iso: Option<String>,
    pub focal_length: Option<String>,
    pub gps: Option<String>,
    pub dpi: Option<String>,
    pub color_space: Option<String>,
}

impl ExifData {
    pub fn read(path: &Path) -> Self {
        let Ok(file) = File::open(path) else {
            return Self::default();
        };
        let Ok(exif) = Reader::new().read_from_container(&mut BufReader::new(file)) else {
            return Self::default();
        };
        Self {
            make: str_field(&exif, Tag::Make),
            model: str_field(&exif, Tag::Model),
            datetime: str_field(&exif, Tag::DateTime),
            exposure_time: exposure_time_str(&exif),
            f_number: f_number_str(&exif),
            iso: str_field(&exif, Tag::PhotographicSensitivity),
            focal_length: focal_length_str(&exif),
            gps: gps_str(&exif),
            dpi: dpi_str(&exif),
            color_space: color_space_str(&exif),
        }
    }
}

fn str_field(exif: &Exif, tag: Tag) -> Option<String> {
    let s = exif
        .get_field(tag, In::PRIMARY)?
        .display_value()
        .to_string();
    let trimmed = s.trim().trim_matches('"');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn rational_field(exif: &Exif, tag: Tag) -> Option<&Field> {
    exif.get_field(tag, In::PRIMARY)
}

fn exposure_time_str(exif: &Exif) -> Option<String> {
    if let Value::Rational(ref v) = rational_field(exif, Tag::ExposureTime)?.value {
        let r = v.first()?;
        if r.num == 0 || r.denom == 0 {
            return None;
        }
        return Some(if r.denom > r.num {
            format!("1/{}", r.denom / r.num)
        } else {
            format!("{:.1}s", r.to_f64())
        });
    }
    None
}

fn f_number_str(exif: &Exif) -> Option<String> {
    if let Value::Rational(ref v) = rational_field(exif, Tag::FNumber)?.value {
        let r = v.first()?;
        if r.denom == 0 {
            return None;
        }
        return Some(format!("f/{:.1}", r.to_f64()));
    }
    None
}

fn focal_length_str(exif: &Exif) -> Option<String> {
    if let Value::Rational(ref v) = rational_field(exif, Tag::FocalLength)?.value {
        let r = v.first()?;
        if r.denom == 0 {
            return None;
        }
        return Some(format!("{:.0} mm", r.to_f64()));
    }
    None
}

fn resolution_rational(exif: &Exif, tag: Tag) -> Option<f64> {
    if let Value::Rational(ref v) = rational_field(exif, tag)?.value {
        let r = v.first()?;
        if r.denom == 0 {
            return None;
        }
        Some(r.to_f64()).filter(|&v| v > 0.5)
    } else {
        None
    }
}

fn dpi_str(exif: &Exif) -> Option<String> {
    let unit = exif
        .get_field(Tag::ResolutionUnit, In::PRIMARY)
        .and_then(|f| {
            if let Value::Short(ref v) = f.value {
                v.first().copied()
            } else {
                None
            }
        })
        .unwrap_or(2);

    if unit == 1 {
        return None;
    }

    let x = resolution_rational(exif, Tag::XResolution)?;
    let y = resolution_rational(exif, Tag::YResolution);
    let suffix = if unit == 3 { "DPCM" } else { "DPI" };

    Some(match y.filter(|&y| (y - x).abs() > 0.5) {
        Some(y) => format!("{:.0} × {:.0} {}", x, y, suffix),
        None => format!("{:.0} {}", x, suffix),
    })
}

fn color_space_str(exif: &Exif) -> Option<String> {
    if let Value::Short(ref v) = exif.get_field(Tag::ColorSpace, In::PRIMARY)?.value {
        match v.first().copied()? {
            1 => Some("sRGB".to_string()),
            65535 => Some("Uncalibrated".to_string()),
            _ => None,
        }
    } else {
        None
    }
}

fn rational_to_degrees(v: &[exif::Rational]) -> Option<f64> {
    if v.len() < 3 || v[0].denom == 0 || v[1].denom == 0 || v[2].denom == 0 {
        return None;
    }
    Some(v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0)
}

fn gps_str(exif: &Exif) -> Option<String> {
    let lat_field = exif.get_field(Tag::GPSLatitude, In::PRIMARY)?;
    let lon_field = exif.get_field(Tag::GPSLongitude, In::PRIMARY)?;

    let lat_ref = exif
        .get_field(Tag::GPSLatitudeRef, In::PRIMARY)
        .map(|f: &Field| f.display_value().to_string())
        .unwrap_or_default();
    let lon_ref = exif
        .get_field(Tag::GPSLongitudeRef, In::PRIMARY)
        .map(|f: &Field| f.display_value().to_string())
        .unwrap_or_default();

    if let (Value::Rational(lv), Value::Rational(lnv)) = (&lat_field.value, &lon_field.value) {
        let mut lat = rational_to_degrees(lv)?;
        let mut lon = rational_to_degrees(lnv)?;
        if lat_ref.contains('S') {
            lat = -lat;
        }
        if lon_ref.contains('W') {
            lon = -lon;
        }
        return Some(format!("{:.4}, {:.4}", lat, lon));
    }
    None
}
