//! Source-specific image decoding behind the shared thumbnail worker.
use super::*;

pub(super) enum Reader {
    D2Modern(Box<parhelion_import::d2_mot::reader::Reader>),
    D1RiseOfIron(Box<parhelion_import::d1_roi::Source>, Option<PathBuf>),
}

impl Reader {
    pub fn open(
        source: Source,
        folder: &Path,
        cache: &Path,
        xg: Option<PathBuf>,
    ) -> Result<Self, String> {
        match source {
            Source::D2Modern => {
                parhelion_import::d2_mot::reader::Reader::discovery(folder, cache, true)
                    .map(|reader| Self::D2Modern(Box::new(reader)))
            }
            Source::D1RiseOfIron => parhelion_import::d1_roi::Source::open(folder, |_, _| {})
                .map(|reader| Self::D1RiseOfIron(Box::new(reader), xg)),
            _ => return Err("This source has no inventory artwork reader".into()),
        }
        .map_err(|error| format!("{error:#}"))
    }

    pub fn read(&mut self, index: usize) -> Result<image::RgbaImage, String> {
        let (size, rgba) = match self {
            Self::D2Modern(reader) => {
                let layers =
                    icon::read_layers(reader, index).map_err(|error| format!("{error:#}"))?;
                composite(&layers)?
            }
            Self::D1RiseOfIron(reader, xg) => {
                let strings = u32::try_from(index).map_err(|error| error.to_string())?;
                let layers = reader
                    .item_icon_layers(strings, xg.as_deref())
                    .map_err(|error| format!("{error:#}"))?;
                composite(&layers)?
            }
        };
        image::RgbaImage::from_raw(size[0] as u32, size[1] as u32, rgba)
            .ok_or_else(|| "Invalid icon pixel buffer".to_owned())
    }
}
