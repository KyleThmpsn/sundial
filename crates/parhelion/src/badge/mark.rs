//! The artwork of an importer's badge: its source's own mark, read from the source game's files
//! the first time and kept in the importer's data folder as `marks/<source>.png`, so later builds
//! draw the same picture without the game.
use crate::presentation::{
    Artwork,
    composition::{Background, Composition},
};
use sundial::package_authoring::mod_content::Importer;

/// The badge artwork of `importer`'s mod, centred at 60% of the card's size on its source's
/// gradient. `None` when its source's mark can't be read, which leaves the badge the Project
/// Sunrise artwork. Test builds read no personal game or data folders, so they always draw that
/// artwork.
pub(crate) fn artwork(importer: Importer) -> Option<Artwork> {
    let image = mark(importer)?;
    Artwork::from_source(image)
        .and_then(|art| {
            art.with_composition(Composition {
                scale: 60,
                background: background(importer),
                ..Default::default()
            })
        })
        .ok()
}

/// A top-to-bottom gradient in each source's colours that keeps its mark readable: navy under the
/// gold Monument of Triumph seal, rust under the white Rise of Iron tricorn, slate under the silver
/// Halo: Reach logo and acid lime under Marathon's dark grey mark.
fn background(importer: Importer) -> Background {
    let (start, end) = match importer {
        Importer::D2Mot => ([30, 52, 88], [9, 16, 31]),
        Importer::D1Roi => ([150, 60, 26], [38, 14, 8]),
        Importer::HaloReach => ([74, 94, 110], [16, 22, 29]),
        Importer::Marathon => ([198, 242, 28], [104, 142, 10]),
    };
    Background::Gradient {
        start,
        end,
        angle: 90,
    }
}

#[cfg(test)]
fn mark(_: Importer) -> Option<image::RgbaImage> {
    None
}

#[cfg(not(test))]
fn mark(importer: Importer) -> Option<image::RgbaImage> {
    let folder = sundial::package_authoring::parhelion_data_directory()?.join("importer");
    let path = folder
        .join("marks")
        .join(format!("{}.png", importer.source()));
    if let Ok(image) = image::open(&path) {
        return Some(image.into_rgba8());
    }
    let image = read(importer, &folder)?;
    let mut bytes = std::io::Cursor::new(Vec::new());
    if image.write_to(&mut bytes, image::ImageFormat::Png).is_ok()
        && std::fs::create_dir_all(path.parent()?).is_ok()
    {
        // A mark that can't be kept is read again next time.
        let _ = sundial::package_authoring::replace_authoring_file(&path, &bytes.into_inner());
    }
    Some(image)
}

#[cfg(all(not(test), feature = "d2-model-importer"))]
fn read(importer: Importer, folder: &std::path::Path) -> Option<image::RgbaImage> {
    let settings = serde_json::from_slice::<parhelion_import::d2_mot::service::Settings>(
        &std::fs::read(folder.join("settings.json")).ok()?,
    )
    .ok()?;
    parhelion_import::mark::read(importer.source(), &settings).ok()
}

#[cfg(all(not(test), not(feature = "d2-model-importer")))]
fn read(_: Importer, _: &std::path::Path) -> Option<image::RgbaImage> {
    None
}
