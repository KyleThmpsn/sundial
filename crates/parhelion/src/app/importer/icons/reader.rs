//! Source-specific image decoding behind the shared thumbnail worker.
use super::*;

pub(super) enum Reader {
    Modern(Box<parhelion_import::d2_mot::reader::Reader>),
    Roi(Box<parhelion_import::d1_roi::Source>, Option<PathBuf>),
}

impl Reader {
    pub fn open(
        source: Source,
        folder: &Path,
        cache: &Path,
        xg: Option<PathBuf>,
    ) -> Result<Self, String> {
        match source {
            Source::Modern => {
                parhelion_import::d2_mot::reader::Reader::discovery(folder, cache, true)
                    .map(|reader| Self::Modern(Box::new(reader)))
            }
            Source::Roi => parhelion_import::d1_roi::Source::open(folder, |_, _| {})
                .map(|reader| Self::Roi(Box::new(reader), xg)),
            _ => return Err("This source has no inventory artwork reader".into()),
        }
        .map_err(|error| format!("{error:#}"))
    }

    pub fn read(&mut self, index: usize) -> Result<image::RgbaImage, String> {
        let (size, rgba) = match self {
            Self::Modern(reader) => {
                let layers =
                    icon::read_layers(reader, index).map_err(|error| format!("{error:#}"))?;
                composite(&layers)?
            }
            Self::Roi(reader, xg) => {
                let strings = u32::try_from(index).map_err(|error| error.to_string())?;
                let icon = reader
                    .item_icon(strings, xg.as_deref())
                    .map_err(|error| format!("{error:#}"))?
                    .ok_or("No source inventory artwork")?;
                (
                    [usize::from(icon.width), usize::from(icon.height)],
                    icon.rgba,
                )
            }
        };
        image::RgbaImage::from_raw(size[0] as u32, size[1] as u32, rgba)
            .ok_or_else(|| "Invalid icon pixel buffer".to_owned())
    }
}
