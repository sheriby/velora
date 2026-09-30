use super::*;


/// Metadata for a selectable theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeCatalogEntry {
    pub id: String,
    pub name: String,
}

pub(crate) const BUILTIN_THEME_DARK_ID: &str = "velora-dark";
pub(crate) const BUILTIN_THEME_DARK_NAME: &str = "Velora Dark";
pub(crate) const BUILTIN_THEME_LIGHT_ID: &str = "velora-light";
pub(crate) const BUILTIN_THEME_LIGHT_NAME: &str = "Velora Light";
pub(crate) const BUILTIN_THEME_PAPER_ID: &str = "paper";
pub(crate) const BUILTIN_THEME_PAPER_NAME: &str = "Paper";
pub(crate) const BUILTIN_THEME_FOREST_ID: &str = "forest";
pub(crate) const BUILTIN_THEME_FOREST_NAME: &str = "Forest";
pub(crate) const BUILTIN_THEME_MIDNIGHT_ID: &str = "midnight";
pub(crate) const BUILTIN_THEME_MIDNIGHT_NAME: &str = "Midnight";
pub(crate) const BUILTIN_THEME_INK_ID: &str = "ink";
pub(crate) const BUILTIN_THEME_INK_NAME: &str = "Ink";
pub(crate) const BUILTIN_THEME_SYSTEM_ID: &str = "system";
const CUSTOM_THEME_ID: &str = "custom";

fn builtin_theme_catalog() -> Vec<ThemeCatalogEntry> {
    vec![
        ThemeCatalogEntry {
            id: BUILTIN_THEME_SYSTEM_ID.into(),
            name: "System".into(),
        },
        ThemeCatalogEntry {
            id: BUILTIN_THEME_DARK_ID.into(),
            name: BUILTIN_THEME_DARK_NAME.into(),
        },
        ThemeCatalogEntry {
            id: BUILTIN_THEME_LIGHT_ID.into(),
            name: BUILTIN_THEME_LIGHT_NAME.into(),
        },
        ThemeCatalogEntry {
            id: BUILTIN_THEME_PAPER_ID.into(),
            name: BUILTIN_THEME_PAPER_NAME.into(),
        },
        ThemeCatalogEntry {
            id: BUILTIN_THEME_FOREST_ID.into(),
            name: BUILTIN_THEME_FOREST_NAME.into(),
        },
        ThemeCatalogEntry {
            id: BUILTIN_THEME_MIDNIGHT_ID.into(),
            name: BUILTIN_THEME_MIDNIGHT_NAME.into(),
        },
        ThemeCatalogEntry {
            id: BUILTIN_THEME_INK_ID.into(),
            name: BUILTIN_THEME_INK_NAME.into(),
        },
    ]
}

#[derive(Debug, Clone)]
pub(crate) struct CustomThemeEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) creator: String,
    pub(crate) base_theme_id: String,
    pub(crate) theme: Theme,
}

/// Global singleton that holds the current [`Theme`].
///
/// Registered via [`Global`] so every component can access it through
/// `cx.global::<ThemeManager>().current()` without passing props.
pub struct ThemeManager {
    current: Arc<Theme>,
    current_theme_id: String,
    system_appearance: WindowAppearance,
    custom_themes: Vec<CustomThemeEntry>,
    theme_catalog: Vec<ThemeCatalogEntry>,
}

impl Global for ThemeManager {}

impl Default for ThemeManager {
    fn default() -> Self {
        Self {
            current: Arc::new(Theme::default_theme()),
            current_theme_id: BUILTIN_THEME_DARK_ID.into(),
            system_appearance: WindowAppearance::Light,
            custom_themes: Vec::new(),
            theme_catalog: builtin_theme_catalog(),
        }
    }
}

#[allow(unused)]
impl ThemeManager {
    /// Installs the configured theme into GPUI's global state.
    pub fn init(cx: &mut App) {
        let theme_id = crate::config::read_app_preferences()
            .map(|preferences| preferences.default_theme_id)
            .unwrap_or_else(|_| BUILTIN_THEME_SYSTEM_ID.into());
        Self::init_with_theme_id(cx, &theme_id);
    }

    /// Installs a specific theme into GPUI's global state.
    pub fn init_with_theme_id(cx: &mut App, theme_id: &str) {
        let mut manager = Self::default();
        if let Ok(dirs) = VeloraConfigDirs::from_system()
            && let Err(err) = manager.load_custom_themes_from_dirs(&dirs)
        {
            eprintln!("failed to load custom themes: {err}");
        }
        if theme_id == BUILTIN_THEME_SYSTEM_ID {
            manager.system_appearance = cx.window_appearance();
        }
        let _ = manager.set_theme_by_id(theme_id);
        cx.set_global(manager);
    }

    /// Returns the currently active theme.
    pub fn current(&self) -> &Theme {
        &self.current
    }

    /// Returns an `Arc` clone of the currently active theme — O(1), no
    /// per-field copy. Use this in hot render paths instead of cloning the
    /// whole `Theme` struct (which has ~200 fields and a `String` name).
    pub fn current_arc(&self) -> Arc<Theme> {
        self.current.clone()
    }

    /// Returns the identifier of the currently active theme.
    pub fn current_theme_id(&self) -> &str {
        &self.current_theme_id
    }

    /// Returns all built-in and imported themes exposed in the native menu.
    pub fn available_themes(&self) -> &[ThemeCatalogEntry] {
        &self.theme_catalog
    }

    /// Colors used by the theme picker before a theme is applied.
    pub fn preview_colors(&self, theme_id: &str) -> Option<(Hsla, Hsla, Hsla)> {
        let theme = match theme_id {
            BUILTIN_THEME_SYSTEM_ID => match self.system_appearance {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::default_theme(),
                WindowAppearance::Light | WindowAppearance::VibrantLight => Theme::light_theme(),
            },
            BUILTIN_THEME_DARK_ID => Theme::default_theme(),
            BUILTIN_THEME_LIGHT_ID => Theme::light_theme(),
            BUILTIN_THEME_PAPER_ID => Theme::paper_theme(),
            BUILTIN_THEME_FOREST_ID => Theme::forest_theme(),
            BUILTIN_THEME_MIDNIGHT_ID => Theme::midnight_theme(),
            BUILTIN_THEME_INK_ID => Theme::ink_theme(),
            _ => self
                .custom_themes
                .iter()
                .find(|entry| entry.id == theme_id)?
                .theme
                .clone(),
        };
        Some((
            theme.colors.editor_background,
            theme.colors.text_default,
            theme.colors.text_link,
        ))
    }

    /// Loads and activates a theme from a file.
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let theme = Theme::from_file(path)?;
        self.current_theme_id = self.theme_id_for_loaded_theme(&theme);
        self.current = Arc::new(theme);
        Ok(())
    }

    /// Loads and activates a theme from JSON text.
    pub fn load_json(&mut self, json: &str) -> anyhow::Result<()> {
        let theme = Theme::from_json(json)?;
        self.current_theme_id = self.theme_id_for_loaded_theme(&theme);
        self.current = Arc::new(theme);
        Ok(())
    }

    /// Replaces the active theme with a fully constructed value.
    pub fn set_theme(&mut self, theme: Theme) {
        self.current_theme_id = self.theme_id_for_loaded_theme(&theme);
        self.current = Arc::new(theme);
    }

    /// Restores the built-in default theme.
    pub fn reset(&mut self) {
        self.current_theme_id = BUILTIN_THEME_SYSTEM_ID.into();
        self.current = Arc::new(match self.system_appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::default_theme(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Theme::light_theme(),
        });
    }

    /// Updates the active system-following theme when the OS appearance changes.
    pub fn set_system_appearance(&mut self, appearance: WindowAppearance) {
        self.system_appearance = appearance;
        if self.current_theme_id == BUILTIN_THEME_SYSTEM_ID {
            self.current = Arc::new(match appearance {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::default_theme(),
                WindowAppearance::Light | WindowAppearance::VibrantLight => Theme::light_theme(),
            });
        }
    }

    /// Activates a theme by identifier.
    pub fn set_theme_by_id(&mut self, theme_id: &str) -> bool {
        match theme_id {
            id if id == BUILTIN_THEME_SYSTEM_ID => {
                self.current_theme_id = BUILTIN_THEME_SYSTEM_ID.into();
                self.current = Arc::new(match self.system_appearance {
                    WindowAppearance::Dark | WindowAppearance::VibrantDark => {
                        Theme::default_theme()
                    }
                    WindowAppearance::Light | WindowAppearance::VibrantLight => {
                        Theme::light_theme()
                    }
                });
                true
            }
            id if id == BUILTIN_THEME_DARK_ID => {
                self.current = Arc::new(Theme::default_theme());
                self.current_theme_id = BUILTIN_THEME_DARK_ID.into();
                true
            }
            id if id == BUILTIN_THEME_LIGHT_ID => {
                self.current = Arc::new(Theme::light_theme());
                self.current_theme_id = BUILTIN_THEME_LIGHT_ID.into();
                true
            }
            id if id == BUILTIN_THEME_PAPER_ID => {
                self.current = Arc::new(Theme::paper_theme());
                self.current_theme_id = BUILTIN_THEME_PAPER_ID.into();
                true
            }
            id if id == BUILTIN_THEME_FOREST_ID => {
                self.current = Arc::new(Theme::forest_theme());
                self.current_theme_id = BUILTIN_THEME_FOREST_ID.into();
                true
            }
            id if id == BUILTIN_THEME_MIDNIGHT_ID => {
                self.current = Arc::new(Theme::midnight_theme());
                self.current_theme_id = BUILTIN_THEME_MIDNIGHT_ID.into();
                true
            }
            id if id == BUILTIN_THEME_INK_ID => {
                self.current = Arc::new(Theme::ink_theme());
                self.current_theme_id = BUILTIN_THEME_INK_ID.into();
                true
            }
            id => {
                let Some(entry) = self.custom_themes.iter().find(|entry| entry.id == id) else {
                    return false;
                };
                self.current = Arc::new(entry.theme.clone());
                self.current_theme_id = entry.id.clone();
                true
            }
        }
    }

    /// Imports a user theme pack, persists a normalized copy, and activates it.
    pub fn import_theme_config(&mut self, path: impl AsRef<Path>) -> anyhow::Result<String> {
        let dirs = VeloraConfigDirs::from_system()?;
        self.import_theme_config_with_dirs(path, &dirs)
    }

    pub(crate) fn import_theme_config_with_dirs(
        &mut self,
        path: impl AsRef<Path>,
        dirs: &VeloraConfigDirs,
    ) -> anyhow::Result<String> {
        let raw = read_json_or_jsonc(path.as_ref())?;
        let default_base_theme_id = self.theme_import_base_theme_id();
        let (entry, normalized) =
            custom_theme_from_value_with_default_base(raw, default_base_theme_id.as_str())?;
        let file_name = format!(
            "{}_{}.json",
            sanitize_config_file_stem(&entry.name),
            sanitize_config_file_stem(&entry.creator)
        );
        let themes_dir = dirs.themes_dir();
        std::fs::create_dir_all(&themes_dir)?;
        std::fs::write(
            themes_dir.join(file_name),
            serde_json::to_string_pretty(&normalized)?,
        )?;
        let imported_id = entry.id.clone();
        self.upsert_custom_theme(entry);
        self.set_theme_by_id(&imported_id);
        Ok(imported_id)
    }

    pub(crate) fn load_custom_themes_from_dirs(&mut self, dirs: &VeloraConfigDirs) -> anyhow::Result<()> {
        let themes_dir = dirs.themes_dir();
        if !themes_dir.exists() {
            return Ok(());
        }

        let mut loaded = Vec::new();
        for entry in std::fs::read_dir(&themes_dir)? {
            let path = entry?.path();
            if path.is_file() {
                match read_json_or_jsonc(&path)
                    .and_then(|value| custom_theme_from_value(value).map(|(entry, _)| entry))
                {
                    Ok(entry) => loaded.push(entry),
                    Err(err) => {
                        eprintln!("skipping custom theme config '{}': {err}", path.display())
                    }
                }
            }
        }
        loaded.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.creator.cmp(&right.creator))
        });
        for entry in loaded {
            self.upsert_custom_theme(entry);
        }
        Ok(())
    }

    fn upsert_custom_theme(&mut self, entry: CustomThemeEntry) {
        if let Some(existing) = self
            .custom_themes
            .iter_mut()
            .find(|existing| existing.id == entry.id)
        {
            *existing = entry;
        } else {
            self.custom_themes.push(entry);
        }
        self.rebuild_theme_catalog();
    }

    fn rebuild_theme_catalog(&mut self) {
        let mut catalog = builtin_theme_catalog();
        catalog.extend(self.custom_themes.iter().map(|entry| ThemeCatalogEntry {
            id: entry.id.clone(),
            name: format!("{} - {}", entry.name, entry.creator),
        }));
        self.theme_catalog = catalog;
    }

    fn theme_id_for_loaded_theme(&self, theme: &Theme) -> String {
        if theme.name == BUILTIN_THEME_DARK_NAME {
            BUILTIN_THEME_DARK_ID.into()
        } else if theme.name == BUILTIN_THEME_LIGHT_NAME {
            BUILTIN_THEME_LIGHT_ID.into()
        } else if theme.name == BUILTIN_THEME_PAPER_NAME {
            BUILTIN_THEME_PAPER_ID.into()
        } else if theme.name == BUILTIN_THEME_FOREST_NAME {
            BUILTIN_THEME_FOREST_ID.into()
        } else if theme.name == BUILTIN_THEME_MIDNIGHT_NAME {
            BUILTIN_THEME_MIDNIGHT_ID.into()
        } else if theme.name == BUILTIN_THEME_INK_NAME {
            BUILTIN_THEME_INK_ID.into()
        } else {
            CUSTOM_THEME_ID.into()
        }
    }

    fn theme_import_base_theme_id(&self) -> String {
        match self.current_theme_id.as_str() {
            BUILTIN_THEME_SYSTEM_ID => match self.system_appearance {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => {
                    BUILTIN_THEME_DARK_ID.into()
                }
                WindowAppearance::Light | WindowAppearance::VibrantLight => {
                    BUILTIN_THEME_LIGHT_ID.into()
                }
            },
            BUILTIN_THEME_LIGHT_ID => BUILTIN_THEME_LIGHT_ID.into(),
            BUILTIN_THEME_DARK_ID => BUILTIN_THEME_DARK_ID.into(),
            BUILTIN_THEME_PAPER_ID => BUILTIN_THEME_PAPER_ID.into(),
            BUILTIN_THEME_FOREST_ID => BUILTIN_THEME_FOREST_ID.into(),
            BUILTIN_THEME_MIDNIGHT_ID => BUILTIN_THEME_MIDNIGHT_ID.into(),
            BUILTIN_THEME_INK_ID => BUILTIN_THEME_INK_ID.into(),
            id => self
                .custom_themes
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| entry.base_theme_id.clone())
                .unwrap_or_else(|| BUILTIN_THEME_DARK_ID.into()),
        }
    }
}

pub(crate) fn custom_theme_from_value(value: Value) -> anyhow::Result<(CustomThemeEntry, Value)> {
    custom_theme_from_value_with_default_base(value, BUILTIN_THEME_DARK_ID)
}

fn custom_theme_from_value_with_default_base(
    mut value: Value,
    default_base_theme_id: &str,
) -> anyhow::Result<(CustomThemeEntry, Value)> {
    prune_empty_json_values(&mut value);
    let Value::Object(mut object) = value else {
        bail!("theme config must be a JSON object");
    };
    let object = object_without_empty_values(std::mem::take(&mut object));
    let name = required_string(&object, "name")?;
    let creator = required_string(&object, "creator")?;
    let base_theme_id = resolved_custom_theme_base_id(&object, default_base_theme_id);
    let raw_theme_patch = object
        .get("theme")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    if !raw_theme_patch.is_object() {
        bail!("field 'theme' must be a JSON object when present");
    }

    let base_theme = custom_theme_base_theme(&base_theme_id);
    let mut merged = serde_json::to_value(base_theme)?;
    let mut theme_patch = filter_json_by_schema(&raw_theme_patch, &merged);
    if let Value::Object(theme_patch_object) = &mut theme_patch {
        theme_patch_object.remove("name");
    }
    merge_non_empty_json_values(&mut merged, &theme_patch);
    if let Value::Object(merged_object) = &mut merged {
        merged_object.insert("name".into(), Value::String(name.clone()));
    }
    let theme: Theme = serde_json::from_value(merged)
        .with_context(|| format!("failed to construct custom theme '{name}'"))?;
    let id = format!(
        "custom:{}_{}",
        sanitize_config_file_stem(&name),
        sanitize_config_file_stem(&creator)
    );
    let mut normalized_object = Map::new();
    normalized_object.insert("name".into(), Value::String(name.clone()));
    normalized_object.insert("creator".into(), Value::String(creator.clone()));
    normalized_object.insert(
        "base_theme_id".into(),
        Value::String(base_theme_id.to_string()),
    );
    for key in ["description", "version", "homepage", "license"] {
        if let Some(value) = object.get(key) {
            normalized_object.insert(key.into(), value.clone());
        }
    }
    if !theme_patch
        .as_object()
        .map(|object| object.is_empty())
        .unwrap_or(false)
    {
        normalized_object.insert("theme".into(), theme_patch);
    }
    let normalized = Value::Object(normalized_object);

    Ok((
        CustomThemeEntry {
            id,
            name,
            creator,
            base_theme_id: base_theme_id.to_string(),
            theme,
        },
        normalized,
    ))
}

fn resolved_custom_theme_base_id<'a>(
    object: &'a Map<String, Value>,
    default_base_theme_id: &'a str,
) -> &'a str {
    object
        .get("base_theme_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| is_builtin_theme_id(value))
        .unwrap_or_else(|| {
            if is_builtin_theme_id(default_base_theme_id) {
                default_base_theme_id
            } else {
                BUILTIN_THEME_DARK_ID
            }
        })
}

fn is_builtin_theme_id(theme_id: &str) -> bool {
    matches!(
        theme_id,
        BUILTIN_THEME_DARK_ID
            | BUILTIN_THEME_LIGHT_ID
            | BUILTIN_THEME_PAPER_ID
            | BUILTIN_THEME_FOREST_ID
            | BUILTIN_THEME_MIDNIGHT_ID
            | BUILTIN_THEME_INK_ID
    )
}

fn custom_theme_base_theme(theme_id: &str) -> Theme {
    match theme_id {
        BUILTIN_THEME_LIGHT_ID => Theme::light_theme(),
        BUILTIN_THEME_PAPER_ID => Theme::paper_theme(),
        BUILTIN_THEME_FOREST_ID => Theme::forest_theme(),
        BUILTIN_THEME_MIDNIGHT_ID => Theme::midnight_theme(),
        BUILTIN_THEME_INK_ID => Theme::ink_theme(),
        _ => Theme::default_theme(),
    }
}

fn filter_json_by_schema(value: &Value, schema: &Value) -> Value {
    match (value, schema) {
        (Value::Object(value_object), Value::Object(schema_object)) => {
            let mut filtered = Map::new();
            for (key, value) in value_object {
                if let Some(schema_value) = schema_object.get(key) {
                    filtered.insert(key.clone(), filter_json_by_schema(value, schema_value));
                }
            }
            Value::Object(filtered)
        }
        (value, _) => value.clone(),
    }
}

fn required_string(object: &Map<String, Value>, key: &str) -> anyhow::Result<String> {
    let Some(value) = object.get(key) else {
        bail!("missing required field '{key}'");
    };
    let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        bail!("field '{key}' must be a non-empty string");
    };
    Ok(text.to_string())
}
