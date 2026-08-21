use std::{
    fmt,
    fs::read_dir,
    path::{Path, PathBuf},
};

pub const SUPPORTED: &[&str] = &[
    "jpg",
    "jpeg",
    "png",
    "gif",
    "tif",
    "tiff",
    "webp",
    "bmp",
    "ico",
    "qoi",
    "pbm",
    "pgm",
    "ppm",
    "tga",
    "dds",
    "ff",
    "hdr",
    "exr",
    "jxl",
    "psd",
    "psb",
    "icns",
    "kra",
    "xcf",
    "svg",
    "svgz",
    "apng",
    "jp2",
    "j2k",
    "j2c",
    "jpx",
    "dcm",
    "dicom",
    "ktx2",
    #[cfg(feature = "heif")]
    "heic",
    #[cfg(feature = "heif")]
    "heif",
    #[cfg(feature = "av")]
    "mp4",
    #[cfg(feature = "av")]
    "m4v",
    #[cfg(feature = "av")]
    "mov",
    #[cfg(feature = "av")]
    "mkv",
    #[cfg(feature = "av")]
    "webm",
    #[cfg(feature = "av")]
    "avi",
    #[cfg(feature = "av")]
    "mpg",
    #[cfg(feature = "av")]
    "mpeg",
    #[cfg(feature = "av")]
    "ts",
    #[cfg(feature = "av")]
    "m2ts",
    #[cfg(feature = "av")]
    "wmv",
    #[cfg(feature = "av")]
    "flv",
    "ari",
    "arw",
    "cr2",
    "cr3",
    "crm",
    "crw",
    "dcr",
    "dcs",
    "dng",
    "erf",
    "fff",
    "iiq",
    "kdc",
    "mef",
    "mos",
    "mrw",
    "nef",
    "nrw",
    "orf",
    "ori",
    "pef",
    "qtk",
    "raf",
    "raw",
    "rw2",
    "rwl",
    "srw",
    "x3f",
    "3fr",
    "fits",
    "fit",
    "fts",
    "eps",
    "ps",
    "epsf",
];

#[derive(Default)]
pub struct Gallery {
    paths: Vec<PathBuf>,
    index: usize,
    file_size: Option<u64>,
}

impl Gallery {
    pub fn new(file_path: &Path) -> Self {
        let parent: &Path = match file_path.parent() {
            Some(p) => p,
            None => return Self::default(),
        };

        let mut paths: Vec<PathBuf> = read_dir(parent)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name();
                let ext = Path::new(&name).extension()?.to_str()?;
                if !SUPPORTED.iter().any(|&s| s.eq_ignore_ascii_case(ext)) {
                    return None;
                }
                let file_type = entry.file_type().ok()?;
                let is_file = if file_type.is_symlink() {
                    entry.path().is_file()
                } else {
                    file_type.is_file()
                };
                is_file.then(|| entry.path())
            })
            .collect();

        paths.sort_unstable();

        let index = paths.iter().position(|p| p == file_path).unwrap_or(0);

        let file_size = std::fs::metadata(file_path).ok().map(|m| m.len());
        Self {
            paths,
            index,
            file_size,
        }
    }

    pub fn filename(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn refresh_file_size(&mut self) {
        self.file_size = self
            .current()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len());
    }

    pub fn set(&mut self, file_path: PathBuf) -> Option<&PathBuf> {
        if let Some(index) = self.paths.iter().position(|p| p == &file_path) {
            self.index = index;
        } else {
            *self = Gallery::new(&file_path);
            return self.current();
        }
        self.refresh_file_size();
        self.current()
    }

    pub fn next(&mut self) -> Option<&PathBuf> {
        if !self.paths.is_empty() {
            self.index = (self.index + 1) % self.paths.len();
            self.refresh_file_size();
        }
        self.current()
    }

    pub fn previous(&mut self) -> Option<&PathBuf> {
        if !self.paths.is_empty() {
            self.index = (self.index + self.paths.len() - 1) % self.paths.len();
            self.refresh_file_size();
        }
        self.current()
    }

    pub fn file_size(&self) -> Option<u64> {
        self.file_size
    }

    pub fn current(&self) -> Option<&PathBuf> {
        self.paths.get(self.index)
    }

    pub fn position(&self) -> usize {
        self.index
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }
}

impl fmt::Debug for Gallery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Gallery ({} items) — index: {}",
            self.paths.len(),
            self.index
        )?;

        for (i, path) in self.paths.iter().enumerate() {
            if i == self.index {
                writeln!(f, "  -> [{}] {:?}", i, path)?;
            } else {
                writeln!(f, "     [{}] {:?}", i, path)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bloom-gallery-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(gallery: &Gallery) -> Vec<String> {
        gallery
            .paths
            .iter()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn only_supported_files_are_listed() {
        let dir = fixture("filter");
        std::fs::write(dir.join("a.jpg"), b"x").unwrap();
        std::fs::write(dir.join("b.PNG"), b"x").unwrap();
        std::fs::write(dir.join("c.txt"), b"x").unwrap();
        std::fs::write(dir.join("noext"), b"x").unwrap();
        std::fs::create_dir(dir.join("d.jpg")).unwrap();

        let gallery = Gallery::new(&dir.join("a.jpg"));

        assert_eq!(names(&gallery), vec!["a.jpg", "b.PNG"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_image_is_listed_like_a_real_one() {
        let dir = fixture("symlink");
        std::fs::write(dir.join("real.jpg"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("real.jpg"), dir.join("linked.jpg")).unwrap();
        std::os::unix::fs::symlink(dir.join("missing.jpg"), dir.join("dangling.jpg")).unwrap();

        let gallery = Gallery::new(&dir.join("real.jpg"));

        assert_eq!(names(&gallery), vec!["linked.jpg", "real.jpg"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn opening_a_symlink_lands_on_that_symlink() {
        let dir = fixture("position");
        std::fs::write(dir.join("a_real.jpg"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("a_real.jpg"), dir.join("z_link.jpg")).unwrap();

        let gallery = Gallery::new(&dir.join("z_link.jpg"));

        assert_eq!(gallery.len(), 2);
        assert_eq!(gallery.position(), 1);
        assert_eq!(
            gallery.current().and_then(|p| p.file_name()).unwrap(),
            "z_link.jpg"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
