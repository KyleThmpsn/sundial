//! Outside programs and libraries each import source needs. An import looks a tool up in this
//! order: the path chosen in Parhelion, its environment variable, then the places the tool
//! normally lives. The importer checks them before an import starts.
use std::{
    path::{Path, PathBuf},
    sync::RwLock,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tool {
    Ffmpeg,
    Decompiler,
    Oodle,
    Vgmstream,
    Dxc,
}

/// Destiny 2: Monument of Triumph decodes Wwise Opus audio and decompiles source shaders.
pub const MONUMENT: &[Tool] = &[Tool::Ffmpeg, Tool::Decompiler];
/// Destiny 1: Rise of Iron decompresses Oodle 3 blocks and untiles Xbox One textures.
pub const RISE_OF_IRON: &[Tool] = &[Tool::Oodle];
/// Halo: Reach decodes FMOD banks for imported weapons, vehicles and maps.
pub const REACH: &[Tool] = &[Tool::Vgmstream];
/// Marathon disassembles DXIL shaders.
pub const MARATHON: &[Tool] = &[Tool::Dxc];

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Decompiler => "3Dmigoto Decompiler",
            Tool::Oodle => "Oodle 3",
            Tool::Vgmstream => "vgmstream",
            Tool::Dxc => "DXC",
        }
    }

    pub fn purpose(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "Audio",
            Tool::Decompiler | Tool::Dxc => "Shaders",
            Tool::Oodle => "Packages",
            Tool::Vgmstream => "Source Audio",
        }
    }

    /// The file a user picks, or for the decompiler the release archive.
    pub fn file(self) -> &'static str {
        match self {
            Tool::Ffmpeg if cfg!(windows) => "ffmpeg.exe",
            Tool::Ffmpeg => "ffmpeg",
            Tool::Decompiler => "cmd_Decompiler-1.3.16.zip",
            Tool::Oodle if cfg!(windows) => "oo2core_3_win64.dll",
            Tool::Oodle => "liblinoodle3.so",
            Tool::Vgmstream if cfg!(windows) => "vgmstream-cli.exe",
            Tool::Vgmstream => "vgmstream-cli",
            Tool::Dxc if cfg!(windows) => "dxc.exe",
            Tool::Dxc => "dxc",
        }
    }

    /// Where to download the tool, when it has a public download.
    pub fn url(self) -> Option<&'static str> {
        match self {
            Tool::Ffmpeg => Some("https://ffmpeg.org/download.html"),
            Tool::Decompiler => Some("https://github.com/bo3b/3Dmigoto/releases/tag/1.3.16"),
            Tool::Vgmstream => Some("https://github.com/vgmstream/vgmstream/releases"),
            Tool::Dxc => Some("https://github.com/microsoft/DirectXShaderCompiler/releases"),
            Tool::Oodle => None,
        }
    }

    /// Where the tool comes from when it has no public download.
    pub fn origin(self) -> Option<&'static str> {
        match self {
            Tool::Oodle => Some("Found in the Shadowkeep install's bin/x64 folder."),
            _ => None,
        }
    }

    /// Optional tools add to an import without blocking it.
    pub fn required(self) -> bool {
        self != Tool::Vgmstream
    }

    fn variable(self) -> Option<&'static str> {
        match self {
            Tool::Oodle => Some("PARHELION_D1_OODLE"),
            Tool::Dxc => Some("PARHELION_DXC"),
            Tool::Ffmpeg | Tool::Decompiler | Tool::Vgmstream => None,
        }
    }

    fn library(self) -> bool {
        self == Tool::Oodle
    }

    /// The folder Parhelion unpacks the tool into when it fetches the tool itself.
    fn folder(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Decompiler => "3dmigoto",
            Tool::Oodle => "oodle",
            Tool::Vgmstream => "vgmstream",
            Tool::Dxc => "dxc",
        }
    }

    /// The import error for a tool that could not be found or started.
    pub fn missing(self) -> String {
        format!(
            "{} not found. Choose {} under Tools in the D2 Importer.",
            self.name(),
            self.file()
        )
    }
}

/// Tool paths chosen in Parhelion, and the Shadowkeep packages whose install carries Oodle 3.
#[derive(Clone, Debug, Default)]
pub struct Paths {
    pub ffmpeg: Option<PathBuf>,
    pub oodle: Option<PathBuf>,
    pub vgmstream: Option<PathBuf>,
    pub dxc: Option<PathBuf>,
    pub native_packages: Option<PathBuf>,
}

impl Paths {
    const NONE: Paths = Paths {
        ffmpeg: None,
        oodle: None,
        vgmstream: None,
        dxc: None,
        native_packages: None,
    };

    fn chosen(&self, tool: Tool) -> Option<&PathBuf> {
        match tool {
            Tool::Ffmpeg => self.ffmpeg.as_ref(),
            Tool::Oodle => self.oodle.as_ref(),
            Tool::Vgmstream => self.vgmstream.as_ref(),
            Tool::Dxc => self.dxc.as_ref(),
            Tool::Decompiler => None,
        }
    }
}

static CONFIGURED: RwLock<Paths> = RwLock::new(Paths::NONE);

/// Where Parhelion keeps `tool` when it fetches or unpacks the tool itself,
/// `importer/tools/<tool>` in its data folder. Only the decompiler is fetched.
pub fn installed(tool: Tool) -> Option<PathBuf> {
    crate::cache::importer_folder().map(|importer| importer.join("tools").join(tool.folder()))
}

/// Replaces the paths later lookups use.
pub fn configure(paths: Paths) {
    *CONFIGURED
        .write()
        .unwrap_or_else(|error| error.into_inner()) = paths;
}

fn configured() -> Paths {
    CONFIGURED
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

/// Places to look, in order. A chosen path or variable is the only place looked.
fn candidates(tool: Tool) -> Vec<PathBuf> {
    let paths = configured();
    if let Some(path) = paths.chosen(tool) {
        return vec![path.clone()];
    }
    if let Some(path) = tool.variable().and_then(std::env::var_os) {
        return vec![PathBuf::from(path)];
    }
    let file = tool.file();
    let mut places = Vec::new();
    if let Some(folder) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_owned))
    {
        places.push(folder.join(file));
    }
    match tool {
        Tool::Oodle => {
            if let Ok(folder) = std::env::current_dir() {
                places.push(folder.join(file));
            }
            if let Some(install) = paths
                .native_packages
                .as_deref()
                .and_then(|packages| packages.canonicalize().ok())
                .and_then(|packages| packages.parent().map(Path::to_owned))
            {
                places.push(install.join("bin/x64").join(file));
            }
        }
        Tool::Ffmpeg | Tool::Vgmstream | Tool::Dxc => {
            if let Some(path) = std::env::var_os("PATH") {
                places.extend(std::env::split_paths(&path).map(|folder| folder.join(file)));
            }
        }
        Tool::Decompiler => {}
    }
    places
}

/// The program to start. Preserve an explicit selection even if it cannot be opened, so
/// execution fails at that path instead of silently running a different program from PATH.
pub fn executable(tool: Tool) -> PathBuf {
    if let Some(path) = configured().chosen(tool) {
        return path.clone();
    }
    if let Some(path) = tool.variable().and_then(std::env::var_os) {
        return PathBuf::from(path);
    }
    candidates(tool)
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(tool.file()))
}

/// Library paths to try loading, ending with the bare name for the system's DLL search.
pub fn libraries(tool: Tool) -> Vec<PathBuf> {
    let mut paths = candidates(tool);
    let chosen =
        configured().chosen(tool).is_some() || tool.variable().and_then(std::env::var_os).is_some();
    if !chosen {
        paths.push(PathBuf::from(tool.file()));
    }
    paths
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Found(PathBuf),
    Missing,
}

/// Finds a tool without running an import. Libraries are loaded and their entry points checked.
pub fn check(tool: Tool) -> Status {
    let found = match tool {
        Tool::Decompiler => crate::d2_mot::native::automatic::decompiler::cached(),
        _ if tool.library() => libraries(tool).into_iter().find(|path| loads(tool, path)),
        _ => candidates(tool).into_iter().find(|path| path.is_file()),
    };
    found.map_or(Status::Missing, Status::Found)
}

/// Each tool of a source with what was found.
pub fn check_all(tools: &[Tool]) -> Vec<(Tool, Status)> {
    tools.iter().map(|&tool| (tool, check(tool))).collect()
}

/// The tool's path when it is found, for an optional use such as map sounds.
pub fn located(tool: Tool) -> Option<PathBuf> {
    match check(tool) {
        Status::Found(path) => Some(path),
        Status::Missing => None,
    }
}

fn loads(tool: Tool, path: &Path) -> bool {
    let symbol: &[u8] = match tool {
        Tool::Oodle => b"OodleLZDecoder_DecodeSome\0",
        _ => return false,
    };
    // SAFETY: Only the presence of an exported symbol is read. The library is unloaded when it
    // drops, and no entry point is called.
    unsafe { libloading::Library::new(path) }
        .is_ok_and(|library| unsafe { library.get::<*const ()>(symbol) }.is_ok())
}

/// Downloads the decompiler now, or unpacks the release archive from `archive`.
pub fn fetch_decompiler(archive: Option<&Path>) -> anyhow::Result<PathBuf> {
    use crate::d2_mot::native::automatic::decompiler;
    match archive {
        Some(archive) => decompiler::install(archive),
        None => decompiler::prepare(&mut |_| {}),
    }
}
