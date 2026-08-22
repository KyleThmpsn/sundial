use std::path::PathBuf;

#[cfg(windows)]
fn windows_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Sundial"))
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_data_dir()
    }
    #[cfg(not(windows))]
    {
        directories::ProjectDirs::from("", "", "Sundial")
            .map(|directories| directories.config_dir().to_path_buf())
    }
}

pub(crate) fn data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_data_dir()
    }
    #[cfg(not(windows))]
    {
        directories::ProjectDirs::from("", "", "Sundial")
            .map(|directories| directories.data_local_dir().to_path_buf())
    }
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        windows_data_dir()
    }
    #[cfg(not(windows))]
    {
        directories::ProjectDirs::from("", "", "Sundial")
            .map(|directories| directories.cache_dir().to_path_buf())
    }
}
