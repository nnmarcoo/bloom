use iced::Theme;

// maybe add last opened image so we can try to open it if they launch with nothing
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
