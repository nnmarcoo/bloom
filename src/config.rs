use std::collections::HashSet;
use std::path::PathBuf;

use iced::Theme;
use serde::{Deserialize, Serialize};

use crate::keybinds::{Keymap, KeymapFile};

pub const UI_SCALE_MIN: f32 = 0.5;
pub const UI_SCALE_MAX: f32 = 3.0;
pub const UI_SCALE_STEP: f32 = 0.1;
pub const UI_SCALE_DEFAULT: f32 = 1.0;

pub const ALL_THEMES: &[Theme] = &[
    Theme::Light,
    Theme::Dark,
    Theme::Dracula,
    Theme::Nord,
    Theme::SolarizedLight,
    Theme::SolarizedDark,
    Theme::GruvboxLight,
    Theme::GruvboxDark,
    Theme::CatppuccinLatte,
    Theme::CatppuccinFrappe,
    Theme::CatppuccinMacchiato,
    Theme::CatppuccinMocha,
    Theme::TokyoNight,
    Theme::TokyoNightStorm,
    Theme::TokyoNightLight,
    Theme::KanagawaWave,
    Theme::KanagawaDragon,
    Theme::KanagawaLotus,
    Theme::Moonfly,
    Theme::Nightfly,
    Theme::Oxocarbon,
    Theme::Ferra,
];

#[derive(Debug, Clone)]
pub struct Config {
    pub theme: Theme,
    pub show_info: bool,
    pub show_edit: bool,
    pub show_checkerboard: bool,
    pub rounded: bool,
    pub decorations: bool,
    pub always_on_top: bool,
    pub autoplay: bool,
    pub mipmap_zoom_out: bool,
    pub smooth_zoom_in: bool,
    pub keymap: Keymap,
    pub info_collapsed: HashSet<String>,
    pub ui_scale: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            show_info: false,
            show_edit: false,
            show_checkerboard: false,
            rounded: true,
            decorations: true,
            always_on_top: false,
            autoplay: true,
            mipmap_zoom_out: true,
            smooth_zoom_in: false,
            keymap: Keymap::default(),
            info_collapsed: HashSet::new(),
            ui_scale: UI_SCALE_DEFAULT,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ConfigFile {
    theme: String,
    show_info: bool,
    #[serde(default)]
    show_edit: bool,
    #[serde(default)]
    show_checkerboard: bool,
    rounded: bool,
    #[serde(default = "default_true")]
    decorations: bool,
    #[serde(default)]
    always_on_top: bool,
    #[serde(default = "default_true")]
    autoplay: bool,
    #[serde(default = "default_true")]
    mipmap_zoom_out: bool,
    #[serde(default)]
    smooth_zoom_in: bool,
    #[serde(default)]
    keybinds: KeymapFile,
    #[serde(default)]
    info_collapsed: Vec<String>,
    #[serde(default = "default_scale")]
    ui_scale: f32,
}

fn default_true() -> bool {
    true
}

fn default_scale() -> f32 {
    UI_SCALE_DEFAULT
}

impl From<&Config> for ConfigFile {
    fn from(c: &Config) -> Self {
        let mut info_collapsed: Vec<String> = c.info_collapsed.iter().cloned().collect();
        info_collapsed.sort();
        Self {
            theme: c.theme.to_string(),
            show_info: c.show_info,
            show_edit: c.show_edit,
            show_checkerboard: c.show_checkerboard,
            rounded: c.rounded,
            decorations: c.decorations,
            always_on_top: c.always_on_top,
            autoplay: c.autoplay,
            mipmap_zoom_out: c.mipmap_zoom_out,
            smooth_zoom_in: c.smooth_zoom_in,
            keybinds: KeymapFile::from(&c.keymap),
            info_collapsed,
            ui_scale: c.ui_scale,
        }
    }
}

impl From<ConfigFile> for Config {
    fn from(f: ConfigFile) -> Self {
        Self {
            theme: theme_from_str(&f.theme),
            show_info: f.show_info,
            show_edit: f.show_edit,
            show_checkerboard: f.show_checkerboard,
            rounded: f.rounded,
            decorations: f.decorations,
            always_on_top: f.always_on_top,
            autoplay: f.autoplay,
            mipmap_zoom_out: f.mipmap_zoom_out,
            smooth_zoom_in: f.smooth_zoom_in,
            keymap: Keymap::from(f.keybinds),
            info_collapsed: f.info_collapsed.into_iter().collect(),
            ui_scale: f.ui_scale,
        }
    }
}

fn theme_from_str(s: &str) -> Theme {
    ALL_THEMES
        .iter()
        .find(|t| t.to_string() == s)
        .cloned()
        .unwrap_or(Theme::Dark)
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("bloom").join("config.toml"))
}

impl Config {
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => return Self::default(),
        };
        toml::from_str::<ConfigFile>(&text)
            .map(Into::into)
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_path() else {
            eprintln!("bloom: could not determine config directory");
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("bloom: could not create config dir: {e}");
                return;
            }
        }
        match toml::to_string_pretty(&ConfigFile::from(self)) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    eprintln!("bloom: failed to write config: {e}");
                }
            }
            Err(e) => eprintln!("bloom: failed to serialize config: {e}"),
        }
    }
}
