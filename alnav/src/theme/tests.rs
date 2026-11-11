//! Tests for theme tokens, style helpers and `theme.toml` overlay parsing.

use super::*;
use crate::palette::Palette;

#[test]
fn test_log_selection_style_is_soft_gray_no_reverse() {
    install(UiTokens::builtin());
    let style = log_selection_style();
    assert_eq!(style.bg, Some(Color::DarkGray));
    assert!(!style.add_modifier.contains(Modifier::REVERSED));
}

#[test]
fn severe_entry_style_uses_theme_error_red() {
    install(UiTokens::builtin());
    let normal = severe_entry_style(false);
    assert_eq!(normal.fg, Some(Color::Red));
    assert!(!normal.add_modifier.contains(Modifier::BOLD));
    let emphatic = severe_entry_style(true);
    assert_eq!(emphatic.fg, Some(Color::Red));
    assert!(emphatic.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn test_candidate_selection_style_soft_gray_with_white_fg() {
    install(UiTokens::builtin());
    let style = candidate_selection_style();
    assert_eq!(style.bg, Some(Color::DarkGray));
    assert_eq!(style.fg, Some(Color::White));
    assert!(!style.add_modifier.contains(Modifier::REVERSED));
    assert!(!style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn test_candidate_match_and_prefix_tokens() {
    install(UiTokens::builtin());
    assert_eq!(candidate_match_style(false).fg, Some(Color::Cyan));
    assert_eq!(candidate_prefix(), format!("{} ", GLYPH_CARET_SEL));
    use crate::picker::PickerMode;
    assert_eq!(
        picker_mode_prefix(&PickerMode::Manage).content,
        format!("{}  ", GLYPH_MODE_MANAGE)
    );
    assert_eq!(
        picker_mode_prefix(&PickerMode::New).content,
        format!("{}  ", GLYPH_MODE_NEW)
    );
    assert_eq!(
        picker_mode_prefix(&PickerMode::Edit { index: 0 }).content,
        format!("{}  ", GLYPH_MODE_EDIT)
    );
    let soft = picker_mode_style();
    assert_eq!(soft.fg, Some(Color::Cyan));
    assert!(soft.add_modifier.contains(Modifier::DIM));
    assert_eq!(soft.bg, None);
}

#[test]
fn dashboard_logo_is_six_line_unicode_block_wordmark() {
    use unicode_width::UnicodeWidthStr;

    assert_eq!(DASHBOARD_LOGO.len(), 6);
    assert!(DASHBOARD_LOGO[0].contains("█████"));
    for line in DASHBOARD_LOGO {
        assert!(!line.is_ascii());
        assert!(line.chars().all(|ch| " █╗╔═║╝╚".contains(ch)));
        assert_eq!(UnicodeWidthStr::width(line), 43);
    }
}

#[test]
fn test_log_visual_style_differs_from_selection() {
    install(UiTokens::builtin());
    assert_ne!(log_visual_style().bg, log_selection_style().bg);
}

#[test]
fn test_chip_group_border_style_distinct_from_region_accent() {
    install(UiTokens::builtin());
    assert_ne!(selection_frame(), accent());
    assert_eq!(chip_group_border_style(true).fg, Some(selection_frame()));
}

#[test]
fn test_chip_pill_style_fill() {
    install(UiTokens::builtin());
    let (text, body) = chip_pill_style(ChipField::Tag, "MyTag", false);
    assert_eq!(text, format!(" {} MyTag ", GLYPH_FIELD_TAG));
    assert_eq!(body.bg, Some(accent()));
}

#[test]
fn parse_named_and_hex_colors() {
    assert_eq!(parse_color("cyan").unwrap(), Color::Cyan);
    assert_eq!(parse_color("#0ff").unwrap(), Color::Rgb(0, 255, 255));
    assert_eq!(
        parse_color("#112233").unwrap(),
        Color::Rgb(0x11, 0x22, 0x33)
    );
    assert!(parse_color("nope").is_err());
}

#[test]
fn parse_theme_toml_partial_override() {
    let t = parse_theme_toml("accent = \"red\"\n").unwrap();
    assert_eq!(t.accent, Color::Red);
    assert_eq!(t.success, Color::Green);
}

#[test]
fn canvas_style_reset_on_default_rgb_on_kanagawa() {
    install(map_to_tokens(&Palette::default_ansi()));
    assert_eq!(canvas_bg(), Color::Reset);
    assert_eq!(canvas_style().bg, None);
    assert_eq!(canvas_style().fg, None);
    let p = crate::theme_builtins::palette_by_name("kanagawa").unwrap();
    install(map_to_tokens(&p));
    assert_eq!(canvas_style().bg, Some(p.background));
    assert_eq!(canvas_style().fg, Some(p.foreground));
    install(UiTokens::builtin());
}

#[test]
fn default_map_keeps_cyan_accent_and_reset_canvas() {
    let t = map_to_tokens(&Palette::default_ansi());
    assert_eq!(t.accent, Color::Cyan);
    assert_eq!(t.canvas_bg, Color::Reset);
    assert_eq!(t.log_selection_bg, Color::DarkGray);
    assert_eq!(t.bookmark_row_bg, Color::Rgb(36, 24, 40));
    assert_eq!(t.pkg, Color::LightYellow);
    assert_eq!(t.highlight[0], Color::Yellow);
    assert!(t.logo.iter().all(|c| *c == Color::Cyan));
}

#[test]
fn kanagawa_map_paints_canvas_and_yellow_highlight0() {
    let p = crate::theme_builtins::palette_by_name("kanagawa").unwrap();
    let t = map_to_tokens_for(&p, "kanagawa");
    assert_eq!(t.canvas_bg, Color::Rgb(0x1f, 0x1f, 0x28));
    assert_eq!(t.highlight[0], p.yellow);
    assert_eq!(t.error, p.red);
    assert_ne!(t.log_selection_bg, Color::DarkGray);
    assert_eq!(t.accent, p.blue);
    assert_eq!(t.candidate_match_fg, p.blue);
    assert_eq!(t.logo[0], p.blue);
    assert_eq!(t.logo[5], p.cyan);
    assert_ne!(t.logo[0], t.logo[5]);
}

#[test]
fn named_themes_use_distinct_signature_accents() {
    let cases: &[(&str, fn(&Palette) -> Color)] = &[
        ("onedark", |p| p.blue),
        ("dracula", |p| p.magenta),
        ("everforest", |p| p.green),
        ("tokyo-night", |p| p.blue),
        ("catppuccin-mocha", |p| p.magenta),
        ("gruvbox-dark", |p| p.yellow),
        ("nord", |p| p.cyan),
        ("kanagawa", |p| p.blue),
    ];
    let mut seen = Vec::new();
    for (name, accent_of) in cases {
        let p = crate::theme_builtins::palette_by_name(name).unwrap();
        let t = map_to_tokens_for(&p, name);
        assert_eq!(t.accent, accent_of(&p), "{name} signature accent");
        assert_ne!(
            t.logo[0], t.logo[5],
            "{name} logo is a ramp, not a flat fill"
        );
        seen.push((name, t.accent));
    }
    assert_ne!(seen[1].1, seen[2].1, "dracula magenta ≠ everforest green");
    assert_ne!(seen[5].1, seen[0].1, "gruvbox yellow ≠ onedark blue");
}

#[test]
fn dashboard_logo_and_muted_follow_installed_named_theme() {
    let p = crate::theme_builtins::palette_by_name("dracula").unwrap();
    install(map_to_tokens_for(&p, "dracula"));
    assert_eq!(dashboard_header_style().fg, Some(p.magenta));
    assert_eq!(dashboard_logo_line_style(0).fg, Some(p.cyan));
    assert_eq!(dashboard_logo_line_style(5).fg, Some(p.blue));
    assert!(dashboard_logo_line_style(0)
        .add_modifier
        .contains(Modifier::BOLD));
    assert_eq!(dashboard_muted_style().fg, Some(p.foreground));
    assert!(dashboard_muted_style().add_modifier.contains(Modifier::DIM));
    install(UiTokens::builtin());
    assert_eq!(dashboard_header_style().fg, Some(Color::Cyan));
    assert!(dashboard_logo_line_style(0)
        .fg
        .iter()
        .all(|c| *c == Color::Cyan));
    assert_eq!(dashboard_muted_style().fg, Some(Color::DarkGray));
}

#[test]
fn overlay_accent_overrides_signature_keeps_logo_ramp() {
    let p = crate::theme_builtins::palette_by_name("kanagawa").unwrap();
    let t = apply_overlay_for(p, "kanagawa", "accent = \"#ff00aa\"\n").unwrap();
    assert_eq!(t.accent, Color::Rgb(0xff, 0x00, 0xaa));
    assert_eq!(t.logo[0], p.blue);
    assert_eq!(t.logo[5], p.cyan);
}

#[test]
fn status_pill_on_has_bg_off_is_dim_without_bg() {
    install(UiTokens::builtin());
    let on = status_pill(GLYPH_FOLLOWING, success());
    assert_eq!(on.style.bg, Some(success()));
    assert_eq!(on.style.fg, Some(contrast_fg(success())));
    assert!(on.style.add_modifier.contains(Modifier::BOLD));
    let off = status_icon_dim(GLYPH_FOLLOWING);
    assert_eq!(off.style.bg, None);
    assert!(off.style.add_modifier.contains(Modifier::DIM));
    assert!(!off.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn disconnect_glyph_shares_source_pill_slot() {
    use unicode_width::UnicodeWidthStr;

    install(UiTokens::builtin());
    // nf-fa-plug. nf-fa-chain-broken (f127) ink is ~2 cells and right-biased
    // in non-Mono Nerd Fonts, so the disconnect pill looked wider/shifted.
    assert_eq!(GLYPH_DISCONNECT, "\u{f1e6}");
    assert_ne!(GLYPH_DISCONNECT, "\u{f127}");
    let disconnect = status_pill(GLYPH_DISCONNECT, warning());
    for src in [GLYPH_SOURCE_HDC, GLYPH_SOURCE_ADB, GLYPH_SOURCE_FILE] {
        let connected = status_pill(src, accent());
        assert_eq!(
            UnicodeWidthStr::width(disconnect.content.as_ref()),
            UnicodeWidthStr::width(connected.content.as_ref()),
            "disconnect pill width must match {src:?} source pill"
        );
    }
}

#[test]
fn status_flash_pill_uses_warning_for_failed() {
    install(UiTokens::builtin());
    let failed = status_flash_pill("YANK FAILED");
    assert_eq!(failed.style.bg, Some(warning()));
    assert!(failed.content.contains("YANK FAILED"));
    let ok = status_flash_pill("EXISTS");
    assert_eq!(ok.style.bg, Some(success()));
    assert_ne!(ok.style.bg, failed.style.bg);
}
