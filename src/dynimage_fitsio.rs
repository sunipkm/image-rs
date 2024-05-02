use std::{path::{Path, PathBuf}, time::{Duration, SystemTime, UNIX_EPOCH}};

use chrono::{DateTime, Utc};
use fitsio::{errors::Error as FitsError, images::{ImageDescription, ImageType}, FitsFile};

use crate::DynamicImage;

#[derive(Debug, Clone, Copy, PartialEq, Hash)]
/// Compression algorithms used in FITS files.
pub enum FitsCompression {
    /// No compression.
    None,
    /// GZIP compression.
    Gzip,
    /// Rice compression.
    Rice,
    /// HCOMPRESS compression.
    Hcompress,
    /// HCOMPRESS with smoothing.
    Hsmooth,
    /// BZIP2 compression.
    Bzip2,
    /// PLIO compression.
    Plio,
}

impl FitsCompression {
    fn extension(&self) -> &str {
        match self {
            FitsCompression::None => "fits",
            FitsCompression::Gzip => "fits[compress G]",
            FitsCompression::Rice => "fits[compress R]",
            FitsCompression::Hcompress => "fits[compress H]",
            FitsCompression::Hsmooth => "fits[compress HS]",
            FitsCompression::Bzip2 => "fits[compress B]",
            FitsCompression::Plio => "fits[compress P]",
        }
    }
}

impl From<Option<FitsCompression>> for FitsCompression {
    fn from(opt: Option<FitsCompression>) -> Self {
        opt.unwrap_or(FitsCompression::None)
    }
}

impl ToString for FitsCompression {
    fn to_string(&self) -> String {
        match self {
            FitsCompression::None => "uncomp",
            FitsCompression::Gzip => "gzip",
            FitsCompression::Rice => "rice",
            FitsCompression::Hcompress => "hcompress",
            FitsCompression::Hsmooth => "hscompress",
            FitsCompression::Bzip2 => "bzip2",
            FitsCompression::Plio => "plio",
        }
        .to_owned()
    }
}

impl DynamicImage {
    /// Save the image data to a FITS file.
    ///
    /// ### Note
    /// If compression is enabled, the compressed image data is stored
    /// in HDU 1 (IMAGE), while the uncompressed data is stored in the
    /// primary HDU. HDU 1 is created only if compression is enabled.
    /// The HDU containing the image also contains all the necessary
    /// metadata. In case compression is enabled, the primary HDU contains
    /// a key `COMPRESSED_IMAGE` with value `T` to indicate that the compressed
    /// image data is present in HDU 1.
    ///
    /// # Arguments
    ///  * `path` - The path to the FITS file.
    ///  * `compress` - Whether to compress the FITS file. Compression uses
    ///    - GZIP,
    ///    - Rice,
    ///    - HCOMPRESS,
    ///    - HCOMPRESS with smoothing,
    ///    - BZIP2, or
    ///    - PLIO algorithms.
    ///  * `overwrite` - Whether to overwrite the file if it already exists.
    ///
    /// # Errors
    ///  * [`fitsio::errors::Error`] with the error description.
    pub fn savefits(
        &self,
        path: &Path,
        compress: FitsCompression,
        overwrite: bool,
    ) -> Result<PathBuf, FitsError> {
        use DynamicImage::*;

        if path.exists() && path.is_dir() {
            return Err(FitsError::Message("Path is a directory".to_string()));
        }
        let cameraname;
        let timestamp = if let Some(metadata) = self.metadata() {
            metadata.timestamp()
        } else {
            SystemTime::now()
        };
        let timestamp: DateTime<Utc> = timestamp.into();
        let timestamp = timestamp.format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
        let ts = if let Some(metadata) = self.metadata() {
            cameraname = metadata.camera_name();
            metadata
                .timestamp()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| FitsError::Message(e.to_string()))?
                .as_millis() as u64
        } else {
            cameraname = "unknown";
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as u64
        };

        let img_desc = ImageDescription {
            data_type: self.image_type(),
            dimensions: &self.image_size(),
        };

        let mut path = PathBuf::from(path);
        path.set_extension((FitsCompression::None).extension()); // Default extension
        if overwrite && path.exists() { // There seems to be a bug in FITSIO, overwrite() the way called here does nothing
            std::fs::remove_file(&path)?;
        }
        path.set_extension(compress.extension());

        let mut fptr = FitsFile::create(path.clone());
        if overwrite {
            fptr = fptr.overwrite();
        }

        if compress == FitsCompression::None {
            fptr = fptr.with_custom_primary(&img_desc);
        }
        let mut fptr = fptr.open()?;

        let hdu = if compress == FitsCompression::None {
            fptr.primary_hdu()?
        } else {
            let hdu = fptr.primary_hdu()?;
            hdu.write_key(&mut fptr, "COMPRESSED_IMAGE", "T")?;
            hdu.write_key(&mut fptr, "COMPRESSION_ALGO", compress.to_string())?;
            fptr.create_image("IMAGE", &img_desc)?
        };

        match self {
            ImageLuma8(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageLumaA8(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageRgb8(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageRgba8(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageLuma16(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageLumaA16(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageRgb16(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageRgba16(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageRgb32F(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
            ImageRgba32F(p) => hdu.write_image(&mut fptr, p.inner_pixels()),
        }?;

        hdu.write_key(&mut fptr, "CAMERA", cameraname)?;
        hdu.write_key(&mut fptr, "DATE-OBS", timestamp.as_str())?;
        hdu.write_key(&mut fptr, "TIMESTAMP", ts)?;
        if let Some(meta) = self.metadata() {
            let (bin_x, bin_y) = meta.binning();
            hdu.write_key(&mut fptr, "XBINNING", bin_x)?;
            hdu.write_key(&mut fptr, "YBINNING", bin_y)?;
            let (xpixsz, ypixsz) = meta.pixel_size();
            hdu.write_key(&mut fptr, "XPIXSZ", xpixsz)?;
            hdu.write_key(&mut fptr, "YPIXSZ", ypixsz)?;
            hdu.write_key(&mut fptr, "EXPTIME", meta.exposure().as_secs_f64())?;
            hdu.write_key(&mut fptr, "CCD-TEMP", meta.temperature())?;
            let (org_x, org_y) = meta.origin();
            hdu.write_key(&mut fptr, "XORIGIN", org_x)?;
            hdu.write_key(&mut fptr, "YORIGIN", org_y)?;
            hdu.write_key(&mut fptr, "OFFSET", meta.offset())?;
            let (gain, min_gain, max_gain) = meta.gain();
            hdu.write_key(&mut fptr, "GAIN", gain)?;
            hdu.write_key(&mut fptr, "GAIN_MIN", min_gain)?;
            hdu.write_key(&mut fptr, "GAIN_MAX", max_gain)?;
            for obj in meta.extended_metadata().iter() {
                hdu.write_key(&mut fptr, &obj.name, obj.value.as_str())?;
            }
        }

        Ok(path)
        }


    fn image_type(&self) -> ImageType {
        use DynamicImage::*;
        match self {
            ImageLuma8(_) | ImageLumaA8(_) | ImageRgb8(_) | ImageRgba8(_) => ImageType::UnsignedByte,
            ImageLuma16(_) | ImageLumaA16(_) | ImageRgb16(_) | ImageRgba16(_) => ImageType::UnsignedShort,

            ImageRgb32F(_) | ImageRgba32F(_) => ImageType::Float,
        }
    }

    fn image_size(&self) -> Vec<usize> {
        let width = self.width();
        let height = self.height();
        use DynamicImage::*;
        let numpix = match self {
            ImageLuma8(_) | ImageLuma16(_) => 1,
            ImageLumaA8(_) | ImageLumaA16(_) => 2,
            ImageRgb8(_) | ImageRgb16(_) => 3,
            ImageRgba8(_) | ImageRgba16(_) => 4,
            ImageRgb32F(_) => 3,
            ImageRgba32F(_) => 4,
        };
        if numpix == 1 {
            vec![height as usize, width as usize]
        } else {
            vec![height as usize, width as usize, numpix]
        }
    }
}

