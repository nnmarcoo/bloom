use std::{
    fmt,
    fs::read_dir,
    path::{Path, PathBuf},
};

pub const SUPPORTED: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "tif", "tiff", "webp", "bmp", "ico", "qoi", "pbm", "pgm", "ppm",
    "tga", "dds", "ff", "hdr", "exr", "jxl", "psd", "icns", "kra", "avif", "svg", "svgz", "apng",
    "jp2", "j2k", "j2c", "jpx", "dcm", "dicom", "ktx2",
];

#[derive(Default)]
pub struct Gallery {
    paths: Vec<PathBuf>,
    index: usize,
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
            .map(|entry| entry.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext_str| SUPPORTED.iter().any(|&s| s.eq_ignore_ascii_case(ext_str)))
                        .unwrap_or(false)
            })
            .collect();

        paths.sort_unstable();

        let index = paths.iter().position(|p| p == file_path).unwrap_or(0);

        Self { paths, index }
    }

    pub fn filename(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn set(&mut self, file_path: PathBuf) -> Option<&PathBuf> {
        if let Some(index) = self.paths.iter().position(|p| p == &file_path) {
            self.index = index;
        } else {
            *self = Gallery::new(&file_path);
        }
        self.current()
    }

    pub fn next(&mut self) -> Option<&PathBuf> {
        if !self.paths.is_empty() {
            self.index = (self.index + 1) % self.paths.len();
        }
        self.current()
    }

    pub fn previous(&mut self) -> Option<&PathBuf> {
        if !self.paths.is_empty() {
            self.index = (self.index + self.paths.len() - 1) % self.paths.len();
        }
        self.current()
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
