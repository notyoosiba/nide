use std::sync::LazyLock;

use ratatui::style::Color;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;

// -----------------------------------------------------------------------------
// Catppuccin Latte Standard Palette
// -----------------------------------------------------------------------------
pub const ROSEWATER: Color = Color::Rgb(0xdc, 0x8a, 0x78);
pub const FLAMINGO: Color = Color::Rgb(0xdd, 0x78, 0x78);
pub const PINK: Color = Color::Rgb(0xea, 0x76, 0xcb);
pub const MAUVE: Color = Color::Rgb(0x88, 0x39, 0xef);
pub const RED: Color = Color::Rgb(0xd2, 0x0f, 0x39);
pub const MAROON: Color = Color::Rgb(0xe6, 0x45, 0x53);
pub const PEACH: Color = Color::Rgb(0xfe, 0x64, 0x0b);
pub const YELLOW: Color = Color::Rgb(0xdf, 0x8e, 0x1d);
pub const GREEN: Color = Color::Rgb(0x40, 0xa0, 0x2b);
pub const TEAL: Color = Color::Rgb(0x17, 0x92, 0x99);
pub const SKY: Color = Color::Rgb(0x04, 0xa5, 0xe5);
pub const SAPPHIRE: Color = Color::Rgb(0x20, 0x9f, 0xb5);
pub const BLUE: Color = Color::Rgb(0x1e, 0x66, 0xf5);
pub const LAVENDER: Color = Color::Rgb(0x72, 0x87, 0xfd);
pub const TEXT: Color = Color::Rgb(0x4c, 0x4f, 0x69);
pub const SUBTEXT1: Color = Color::Rgb(0x5c, 0x5f, 0x77);
pub const SUBTEXT0: Color = Color::Rgb(0x6c, 0x6f, 0x85);
pub const OVERLAY2: Color = Color::Rgb(0x7c, 0x7f, 0x93);
pub const OVERLAY1: Color = Color::Rgb(0x8c, 0x8f, 0xa1);
pub const OVERLAY0: Color = Color::Rgb(0x9c, 0xa0, 0xb0);
pub const SURFACE2: Color = Color::Rgb(0xac, 0xb0, 0xbe);
pub const SURFACE1: Color = Color::Rgb(0xbc, 0xc0, 0xcc);
pub const SURFACE0: Color = Color::Rgb(0xcc, 0xd0, 0xda);
pub const BASE: Color = Color::Rgb(0xef, 0xf1, 0xf5);
pub const MANTLE: Color = Color::Rgb(0xe6, 0xe9, 0xef);
pub const CRUST: Color = Color::Rgb(0xdc, 0xe0, 0xe8);

// -----------------------------------------------------------------------------
// Semantic UI Theme Constants
// -----------------------------------------------------------------------------
pub const BORDER_ACTIVE: Color = BLUE;
pub const BORDER_INACTIVE: Color = SURFACE2;
pub const STATUS_BG: Color = MANTLE;
pub const STATUS_FG: Color = TEXT;
pub const STATUS_ACCENT: Color = BLUE;
pub const SELECTION_BG: Color = SURFACE1;
pub const SELECTION_FG: Color = TEXT;

pub const SEARCH_MATCH_BG: Color = YELLOW;
pub const SEARCH_MATCH_FG: Color = BASE;
pub const SEARCH_MATCH_CURRENT_BG: Color = PEACH;
pub const SEARCH_MATCH_CURRENT_FG: Color = BASE;

pub const GIT_MODIFIED: Color = YELLOW;
pub const GIT_ADDED: Color = GREEN;
pub const GIT_UNTRACKED: Color = GREEN;
pub const GIT_DELETED: Color = RED;
pub const GIT_IGNORED: Color = OVERLAY0;

pub const DIFF_ADDED: Color = GREEN;
pub const DIFF_CHANGED: Color = BLUE;
pub const DIFF_DELETED: Color = RED;
pub const DIFF_GUTTER_BAR: &str = "▎";

#[must_use]
pub fn git_status_color(status: char) -> Option<Color> {
    match status {
        'M' => Some(GIT_MODIFIED),
        'A' => Some(GIT_ADDED),
        '?' => Some(GIT_UNTRACKED),
        'D' => Some(GIT_DELETED),
        '!' => Some(GIT_IGNORED),
        _ => None,
    }
}


// -----------------------------------------------------------------------------
// Syntect Highlighting & Catppuccin Latte Theme Mapping
// -----------------------------------------------------------------------------
pub static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
pub static THEME_SET: LazyLock<two_face::theme::EmbeddedLazyThemeSet> =
    LazyLock::new(two_face::theme::extra);

#[must_use]
pub fn catppuccin_latte_theme() -> &'static Theme {
    THEME_SET.get(two_face::theme::EmbeddedThemeName::CatppuccinLatte)
}

#[must_use]
pub fn syntect_style_to_ratatui(style: syntect::highlighting::Style) -> ratatui::style::Style {
    let fg = if style.foreground.a == 0 {
        TEXT
    } else {
        Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b)
    };
    let mut s = ratatui::style::Style::default().fg(fg);
    if style.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
        s = s.add_modifier(ratatui::style::Modifier::BOLD);
    }
    if style.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
        s = s.add_modifier(ratatui::style::Modifier::ITALIC);
    }
    if style.font_style.contains(syntect::highlighting::FontStyle::UNDERLINE) {
        s = s.add_modifier(ratatui::style::Modifier::UNDERLINED);
    }
    s
}

#[must_use]
pub fn scope_to_catppuccin(scope_name: &str) -> Color {
    if scope_name.starts_with("keyword") || scope_name.starts_with("storage") {
        MAUVE
    } else if scope_name.starts_with("string") {
        GREEN
    } else if scope_name.starts_with("comment") {
        OVERLAY2
    } else if scope_name.starts_with("entity.name.function")
        || scope_name.starts_with("support.function")
    {
        BLUE
    } else if scope_name.starts_with("constant") {
        PEACH
    } else if scope_name.starts_with("variable") {
        TEXT
    } else if scope_name.starts_with("entity.name.type")
        || scope_name.starts_with("support.type")
    {
        YELLOW
    } else {
        TEXT
    }
}

/// Maps Tree-sitter query capture names (e.g. `@keyword`, `@function`, `@type`,
/// `@string`, `@comment`, `@variable`, `@constant`) to Catppuccin Latte styles.
#[must_use]
pub fn treesitter_capture_to_style(capture: &str) -> Option<ratatui::style::Style> {
    let name = capture.strip_prefix('@').unwrap_or(capture);

    if name.starts_with("keyword") || name.starts_with("storage") || name.starts_with("tag") {
        Some(ratatui::style::Style::default().fg(MAUVE))
    } else if name.starts_with("function") || name.starts_with("method") {
        Some(ratatui::style::Style::default().fg(BLUE))
    } else if name.starts_with("type") || name.starts_with("class") || name.starts_with("struct") {
        Some(ratatui::style::Style::default().fg(YELLOW))
    } else if name.starts_with("string") || name.starts_with("character") {
        Some(ratatui::style::Style::default().fg(GREEN))
    } else if name.starts_with("comment") {
        Some(
            ratatui::style::Style::default()
                .fg(OVERLAY2)
                .add_modifier(ratatui::style::Modifier::ITALIC),
        )
    } else if name.starts_with("constant")
        || name.starts_with("number")
        || name.starts_with("boolean")
        || name.starts_with("float")
    {
        Some(ratatui::style::Style::default().fg(PEACH))
    } else if name.starts_with("variable.parameter") {
        Some(ratatui::style::Style::default().fg(MAROON))
    } else if name.starts_with("variable.builtin") {
        Some(ratatui::style::Style::default().fg(RED))
    } else if name.starts_with("variable") {
        Some(ratatui::style::Style::default().fg(TEXT))
    } else if name.starts_with("operator") {
        Some(ratatui::style::Style::default().fg(SKY))
    } else if name.starts_with("punctuation") {
        Some(ratatui::style::Style::default().fg(SUBTEXT0))
    } else if name.starts_with("property")
        || name.starts_with("field")
        || name.starts_with("attribute")
    {
        Some(ratatui::style::Style::default().fg(TEAL))
    } else {
        None
    }
}

