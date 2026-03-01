use std::path::PathBuf;

use iced::Theme;
use serde::{Deserialize, Serialize};

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
    pub lanczos: bool,
    pub show_info: bool,
    pub rounded: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            lanczos: false,
            show_info: false,
            rounded: true,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ConfigFile {
    theme: String,
    lanczos: bool,
    show_info: bool,
    rounded: bool,
}

impl From<&Config> for ConfigFile {
    fn from(c: &Config) -> Self {
        Self {
            theme: c.theme.to_string(),
            lanczos: c.lanczos,
            show_info: c.show_info,
            rounded: c.rounded,
        }
    }
}

impl From<ConfigFile> for Config {
    fn from(f: ConfigFile) -> Self {
        Self {
            theme: theme_from_str(&f.theme),
            lanczos: f.lanczos,
            show_info: f.show_info,
            rounded: f.rounded,
        }
    }
}

fn theme_from_str(s: &str) -> Theme {
    match s {
        "Light" => Theme::Light,
        "Dark" => Theme::Dark,
        "Dracula" => Theme::Dracula,
        "Nord" => Theme::Nord,
        "Solarized Light" => Theme::SolarizedLight,
        "Solarized Dark" => Theme::SolarizedDark,
        "Gruvbox Light" => Theme::GruvboxLight,
        "Gruvbox Dark" => Theme::GruvboxDark,
        "Catppuccin Latte" => Theme::CatppuccinLatte,
        "Catppuccin Frappé" => Theme::CatppuccinFrappe,
        "Catppuccin Macchiato" => Theme::CatppuccinMacchiato,
        "Catppuccin Mocha" => Theme::CatppuccinMocha,
        "Tokyo Night" => Theme::TokyoNight,
        "Tokyo Night Storm" => Theme::TokyoNightStorm,
        "Tokyo Night Light" => Theme::TokyoNightLight,
        "Kanagawa Wave" => Theme::KanagawaWave,
        "Kanagawa Dragon" => Theme::KanagawaDragon,
        "Kanagawa Lotus" => Theme::KanagawaLotus,
        "Moonfly" => Theme::Moonfly,
        "Nightfly" => Theme::Nightfly,
        "Oxocarbon" => Theme::Oxocarbon,
        "Ferra" => Theme::Ferra,
        _ => Theme::Dark,
    }
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
