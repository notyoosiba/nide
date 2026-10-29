use std::path::{Path, PathBuf};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum WidthsConfig {
    Array([u16; 3]),
    Table {
        #[serde(default)]
        tree: Option<u16>,
        #[serde(default)]
        middle: Option<u16>,
        #[serde(default)]
        right: Option<u16>,
    },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct Config {
    #[serde(default)]
    pub autosave_delay_ms: Option<u64>,
    #[serde(default)]
    pub widths: Option<WidthsConfig>,
    #[serde(default)]
    pub tree_weight: Option<u16>,
    #[serde(default)]
    pub middle_weight: Option<u16>,
    #[serde(default)]
    pub right_weight: Option<u16>,
}

impl Config {
    pub const DEFAULT_AUTOSAVE_DELAY_MS: u64 = 1000;
    pub const DEFAULT_WIDTHS: [u16; 3] = [20, 50, 30];

    #[must_use]
    pub fn autosave_delay(&self) -> std::time::Duration {
        std::time::Duration::from_millis(
            self.autosave_delay_ms.unwrap_or(Self::DEFAULT_AUTOSAVE_DELAY_MS),
        )
    }

    #[must_use]
    pub fn initial_widths(&self) -> [u16; 3] {
        if let Some(widths) = &self.widths {
            match widths {
                WidthsConfig::Array(arr) => *arr,
                WidthsConfig::Table { tree, middle, right } => [
                    tree.unwrap_or(Self::DEFAULT_WIDTHS[0]),
                    middle.unwrap_or(Self::DEFAULT_WIDTHS[1]),
                    right.unwrap_or(Self::DEFAULT_WIDTHS[2]),
                ],
            }
        } else if self.tree_weight.is_some()
            || self.middle_weight.is_some()
            || self.right_weight.is_some()
        {
            [
                self.tree_weight.unwrap_or(Self::DEFAULT_WIDTHS[0]),
                self.middle_weight.unwrap_or(Self::DEFAULT_WIDTHS[1]),
                self.right_weight.unwrap_or(Self::DEFAULT_WIDTHS[2]),
            ]
        } else {
            Self::DEFAULT_WIDTHS
        }
    }

    pub fn config_path() -> Option<PathBuf> {
        if let Some(home) = dirs::home_dir() {
            let p = home.join(".config").join("nide").join("config.toml");
            if p.exists() {
                return Some(p);
            }
        }
        if let Some(config) = dirs::config_dir() {
            let p = config.join("nide").join("config.toml");
            if p.exists() {
                return Some(p);
            }
        }
        dirs::home_dir().map(|h| h.join(".config").join("nide").join("config.toml"))
    }

    #[must_use]
    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(cfg) = toml::from_str::<Config>(&content) {
                        return cfg;
                    }
                }
            }
        }
        Self::default()
    }

    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let cfg = toml::from_str(&content)?;
        Ok(cfg)
    }

    pub fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.autosave_delay(), std::time::Duration::from_millis(1000));
        assert_eq!(cfg.initial_widths(), [20, 50, 30]);
    }

    #[test]
    fn test_config_from_toml_array_widths() {
        let toml_str = r#"
            autosave_delay_ms = 500
            widths = [25, 45, 30]
        "#;
        let cfg = Config::from_toml(toml_str).expect("parse toml");
        assert_eq!(cfg.autosave_delay(), std::time::Duration::from_millis(500));
        assert_eq!(cfg.initial_widths(), [25, 45, 30]);
    }

    #[test]
    fn test_config_from_toml_table_widths() {
        let toml_str = r#"
            autosave_delay_ms = 2500
            [widths]
            tree = 15
            middle = 60
            right = 25
        "#;
        let cfg = Config::from_toml(toml_str).expect("parse toml");
        assert_eq!(cfg.autosave_delay(), std::time::Duration::from_millis(2500));
        assert_eq!(cfg.initial_widths(), [15, 60, 25]);
    }

    #[test]
    fn test_config_from_file_in_tempdir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "autosave_delay_ms = 750\nwidths = [18, 52, 30]\n").unwrap();

        let cfg = Config::from_file(&path).expect("from_file");
        assert_eq!(cfg.autosave_delay(), std::time::Duration::from_millis(750));
        assert_eq!(cfg.initial_widths(), [18, 52, 30]);
    }
}
