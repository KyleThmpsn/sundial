//! Local icon library and opt-in download. Only a validated selection enters a recipe.
use super::{Icon, Purpose};
use crate::icon_edit::ImportedIcon;
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) const REPOSITORY: &str = "https://github.com/justrealmilk/destiny-icons";
const REVISION: &str = "394ed051455e938f72ddd600d42cf87600ec7172";
const MAX_ARCHIVE: u64 = 32 * 1024 * 1024;
const MAX_ICON: u64 = 1024 * 1024;

pub(crate) fn directory() -> Result<PathBuf, String> {
    sundial::package_authoring::parhelion_data_directory()
        .map(|p| p.join("icons"))
        .ok_or_else(|| "Could not locate the icon library.".into())
}

pub(crate) fn downloaded(root: &Path) -> bool {
    root.join("destiny-icons/.complete").is_file()
}

pub(crate) struct Entry {
    pub name: String,
    pub white: bool,
    pub path: PathBuf,
    pub size: [u32; 2],
    pub icon: Icon,
    pub thumbnail: egui::ColorImage,
}

fn png(bytes: &[u8], purpose: Purpose) -> Result<image::RgbaImage, String> {
    let format = if purpose == Purpose::Badge {
        image::guess_format(bytes).map_err(|e| e.to_string())?
    } else {
        image::ImageFormat::Png
    };
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return Err(purpose.guidance().into());
    }
    let image = crate::image_import::decode(bytes, format, purpose.max_edge())?;
    let (w, h) = image.dimensions();
    if !purpose.accepts_size(w, h)
        || (purpose.transparent() && !image.pixels().any(|p| p[3] < 255))
        || !image.pixels().any(|p| p[3] > 0)
        || (purpose == Purpose::Perk
            && super::perk_quality::inspect(image.pixels().map(|p| p.0)).is_none())
    {
        return Err(purpose.guidance().into());
    }
    Ok(image)
}

fn svg(bytes: &[u8], edge: u32) -> Result<image::RgbaImage, String> {
    let options = resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &options).map_err(|e| e.to_string())?;
    let mut pixels = resvg::tiny_skia::Pixmap::new(edge, edge).ok_or("Could not allocate icon")?;
    let scale = edge as f32 / tree.size().width().max(tree.size().height());
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(
        (edge as f32 - tree.size().width() * scale) / 2.0,
        (edge as f32 - tree.size().height() * scale) / 2.0,
    );
    resvg::render(&tree, transform, &mut pixels.as_mut());
    // This icon set consists of monochrome glyphs. Use white, preserving coverage.
    let rgba = pixels
        .data()
        .chunks_exact(4)
        .flat_map(|p| [255, 255, 255, p[3]])
        .collect();
    let image = image::RgbaImage::from_raw(edge, edge, rgba).ok_or("Invalid SVG raster")?;
    if !image.pixels().any(|p| p[3] > 0) {
        return Err("SVG contains no visible artwork".into());
    }
    Ok(image)
}

fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "Icon exceeds the {} MiB limit.",
            limit / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

pub(crate) fn load(path: &Path, purpose: Purpose) -> Result<image::RgbaImage, String> {
    let bytes = bounded_read(path, purpose.max_bytes())?;
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        let image = svg(&bytes, purpose.svg_edge())?;
        if purpose == Purpose::Perk
            && super::perk_quality::inspect(image.pixels().map(|p| p.0)).is_none()
        {
            return Err(purpose.guidance().into());
        }
        Ok(image)
    } else {
        png(&bytes, purpose)
    }
}

fn entry(root: &Path, path: PathBuf, purpose: Purpose) -> Result<Entry, String> {
    let pixels = load(&path, purpose)?;
    let mut encoded = Cursor::new(Vec::new());
    crate::image_import::fit(&pixels, 96, 96)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let name = path
        .strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned();
    let thumbnail = crate::image_import::fit(&pixels, 64, 64);
    Ok(Entry {
        icon: Icon::Image {
            name: name.clone(),
            image: ImportedIcon::from_bytes(encoded.get_ref())?,
        },
        name,
        white: super::perk_quality::inspect(pixels.pixels().map(|p| p.0)) == Some(true),
        path,
        size: [pixels.width(), pixels.height()],
        thumbnail: egui::ColorImage::from_rgba_unmultiplied([64, 64], thumbnail.as_raw()),
    })
}

pub(crate) fn scan(root: &Path, purpose: Purpose) -> Result<(Vec<Entry>, usize), String> {
    if !root.exists() {
        return Ok((vec![], 0));
    }
    let mut dirs = vec![root.to_owned()];
    let mut entries = Vec::new();
    let mut skipped = 0;
    while let Some(dir) = dirs.pop() {
        for child in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let child = child.map_err(|e| e.to_string())?;
            let ty = child.file_type().map_err(|e| e.to_string())?;
            if ty.is_symlink() {
                continue;
            }
            let path = child.path();
            if ty.is_dir() && !child.file_name().to_string_lossy().starts_with('.') {
                dirs.push(path);
            } else if ty.is_file()
                && path.extension().is_some_and(|e| {
                    ["png", "svg", "jpg", "jpeg"]
                        .iter()
                        .any(|kind| e.eq_ignore_ascii_case(kind))
                })
            {
                match entry(root, path, purpose) {
                    Ok(entry) => entries.push(entry),
                    Err(_) => skipped += 1,
                }
            }
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((entries, skipped))
}

pub(crate) fn add(root: &Path, path: &Path, purpose: Purpose) -> Result<Entry, String> {
    let bytes = bounded_read(path, purpose.max_bytes())?;
    png(&bytes, purpose)?;
    let extension = if image::guess_format(&bytes).ok() == Some(image::ImageFormat::Jpeg) {
        "jpg"
    } else {
        "png"
    };
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let name = path
        .file_stem()
        .ok_or("Icon has no filename")?
        .to_string_lossy();
    for index in 0..10000 {
        let destination = root.join(if index == 0 {
            format!("{name}.{extension}")
        } else {
            format!("{name}-{index}.{extension}")
        });
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(mut file) => {
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|e| e.to_string())?;
                return entry(root, destination, purpose);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Too many icons have this filename.".into())
}

pub(crate) fn download(root: &Path) -> Result<(), String> {
    if downloaded(root) {
        return Ok(());
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(format!(
            "https://codeload.github.com/justrealmilk/destiny-icons/zip/{REVISION}"
        ))
        .call()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_ARCHIVE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_ARCHIVE {
        return Err("Icon download exceeds the size limit.".into());
    }
    install_archive(root, &bytes)
}

fn install_archive(root: &Path, bytes: &[u8]) -> Result<(), String> {
    let staging = tempfile::Builder::new()
        .prefix(".destiny-icons-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if archive.len() > 4096 {
        return Err("Icon archive has too many files.".into());
    }
    let mut total = 0;
    let mut count = 0;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let enclosed = file
            .enclosed_name()
            .ok_or("Icon archive contains an unsafe path")?;
        let relative: PathBuf = enclosed.components().skip(1).collect();
        if file.is_dir() || relative.as_os_str().is_empty() {
            continue;
        }
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Icon archive contains a link".into());
        }
        let is_svg = relative.extension().is_some_and(|e| e == "svg");
        if !is_svg && relative != Path::new("LICENSE") && relative != Path::new("readme.md") {
            continue;
        }
        if relative.starts_with("docs-build") {
            continue;
        }
        total += file.size();
        if file.size() > MAX_ICON || total > MAX_ARCHIVE {
            return Err("Icon archive exceeds the size limit.".into());
        }
        let mut contents = Vec::new();
        file.by_ref()
            .take(MAX_ICON + 1)
            .read_to_end(&mut contents)
            .map_err(|e| e.to_string())?;
        if contents.len() as u64 > MAX_ICON {
            return Err("Icon file exceeds the size limit.".into());
        }
        if is_svg {
            svg(&contents, 96)?;
            count += 1;
        }
        let target = staging.path().join(relative);
        fs::create_dir_all(target.parent().ok_or("Invalid icon path")?)
            .map_err(|e| e.to_string())?;
        fs::write(target, contents).map_err(|e| e.to_string())?;
    }
    if count == 0 {
        return Err("The download contains no icons.".into());
    }
    fs::write(staging.path().join(".complete"), REVISION).map_err(|e| e.to_string())?;
    let target = root.join("destiny-icons");
    if target.exists() {
        if downloaded(root) {
            return Ok(());
        }
        return Err(
            "An incomplete destiny-icons folder already exists. Move it aside and retry.".into(),
        );
    }
    fs::rename(staging.path(), target).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_png_is_saved_without_overwriting_and_embedded() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.png");
        let mut image = image::RgbaImage::new(96, 96);
        for y in 24..72 {
            for x in 24..72 {
                image.put_pixel(x, y, image::Rgba([255; 4]));
            }
        }
        image.save(&input).unwrap();
        let root = dir.path().join("icons");
        let first = add(&root, &input, Purpose::Perk).unwrap();
        let second = add(&root, &input, Purpose::Perk).unwrap();
        assert_ne!(first.path, second.path);
        fs::remove_file(&input).unwrap();
        let json = serde_json::to_vec(&first.icon).unwrap();
        assert_eq!(serde_json::from_slice::<Icon>(&json).unwrap(), first.icon);
        assert_eq!(scan(&root, Purpose::Perk).unwrap().0.len(), 2);
    }
    #[test]
    fn svg_is_transparent_white_artwork() {
        let image = svg(br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><path d="M8 8h16v16H8z"/></svg>"#, 96).unwrap();
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert_eq!(image.get_pixel(48, 48).0, [255; 4]);
    }

    #[test]
    fn artwork_sources_keep_resolution_and_use_destination_rules() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("wide.png");
        let mut pixels = image::RgbaImage::from_pixel(768, 256, image::Rgba([180, 90, 30, 255]));
        pixels.save(&input).unwrap();
        assert!(load(&input, Purpose::Perk).is_err());
        assert!(load(&input, Purpose::Watermark).is_err());
        assert_eq!(
            load(&input, Purpose::Badge).unwrap().dimensions(),
            (768, 256)
        );
        pixels.put_pixel(0, 0, image::Rgba([0; 4]));
        pixels.save(&input).unwrap();
        let root = dir.path().join("icons");
        let entry = add(&root, &input, Purpose::Watermark).unwrap();
        assert_eq!(load(&entry.path, Purpose::Watermark).unwrap(), pixels);
        assert_eq!(scan(&root, Purpose::Perk).unwrap().0.len(), 0);
        assert_eq!(scan(&root, Purpose::Badge).unwrap().0.len(), 1);
        assert_eq!(scan(&root, Purpose::Watermark).unwrap().0.len(), 1);
        let glyph = br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><path d="M8 8h16v16H8z"/></svg>"#;
        assert_eq!(
            svg(glyph, Purpose::Badge.svg_edge()).unwrap().dimensions(),
            (512, 512)
        );
    }

    fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in files {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[test]
    fn download_installs_only_complete_validated_archives() {
        let dir = tempfile::tempdir().unwrap();
        let glyph = br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><path d="M8 8h16v16H8z"/></svg>"#;
        let bytes = archive(&[
            ("repo/weapons/example.svg", glyph),
            ("repo/LICENSE", b"CC0"),
        ]);
        install_archive(dir.path(), &bytes).unwrap();
        assert!(downloaded(dir.path()));
        assert!(dir.path().join("destiny-icons/LICENSE").is_file());
        assert_eq!(scan(dir.path(), Purpose::Perk).unwrap().0.len(), 1);
        install_archive(dir.path(), &bytes).unwrap();
        let bad = tempfile::tempdir().unwrap();
        let bytes = archive(&[("repo/../escape.svg", glyph)]);
        assert!(install_archive(bad.path(), &bytes).is_err());
        assert!(!downloaded(bad.path()));
        let bytes = archive(&[("repo/broken.svg", b"invalid")]);
        assert!(install_archive(bad.path(), &bytes).is_err());
        assert!(!downloaded(bad.path()));
    }

    #[test]
    #[ignore = "downloads the pinned public destiny-icons archive to a temporary directory"]
    fn real_destiny_icons_download_and_rasterize() {
        let dir = tempfile::tempdir().unwrap();
        download(dir.path()).unwrap();
        assert!(downloaded(dir.path()));
        let (entries, skipped) = scan(dir.path(), Purpose::Perk).unwrap();
        assert!(entries.len() > 100);
        for entry in &entries {
            let pixels = load(&entry.path, Purpose::Perk).unwrap();
            assert!(super::super::perk_quality::accepts(
                pixels.pixels().map(|p| p.0)
            ));
        }
        eprintln!(
            "Validated {} downloaded perk icons, filtered {skipped}",
            entries.len()
        );
    }
}
