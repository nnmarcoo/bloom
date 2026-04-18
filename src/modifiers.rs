use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModifierType {
    Levels,
    Mosaic,
}

impl fmt::Display for ModifierType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModifierType::Levels => write!(f, "Levels"),
            ModifierType::Mosaic => write!(f, "Mosaic"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Modifier {
    pub kind: ModifierKind,
    pub enabled: bool,
    pub expanded: bool,
}

impl Modifier {
    pub fn new(kind: ModifierKind) -> Self {
        Self {
            kind,
            enabled: true,
            expanded: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ModifierKind {
    Levels {
        shadows: f32,
        midtones: f32,
        highlights: f32,
    },
    Mosaic {
        size: u32,
    },
}

impl ModifierKind {
    pub fn name(&self) -> &'static str {
        match self {
            ModifierKind::Levels { .. } => "Levels",
            ModifierKind::Mosaic { .. } => "Mosaic",
        }
    }
}

impl From<ModifierType> for ModifierKind {
    fn from(t: ModifierType) -> Self {
        match t {
            ModifierType::Levels => ModifierKind::Levels {
                shadows: 0.0,
                midtones: 1.0,
                highlights: 1.0,
            },
            ModifierType::Mosaic => ModifierKind::Mosaic { size: 10 },
        }
    }
}

#[derive(Debug, Clone)]
pub enum ModifierParam {
    LevelsShadows(f32),
    LevelsMidtones(f32),
    LevelsHighlights(f32),
    MosaicSize(u32),
}
