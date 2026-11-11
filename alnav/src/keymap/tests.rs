use crossterm::event::KeyModifiers;

use super::*;

#[test]
fn parse_stroke_modifiers_and_reject_bare_upper() {
    let s = KeyStroke::parse("C-s").unwrap();
    assert!(s.ctrl);
    assert_eq!(s.code, StrokeCode::Char('s'));
    let s = KeyStroke::parse("S-j").unwrap();
    assert!(s.shift);
    assert!(KeyStroke::parse("J").is_err());
    assert_eq!(
        KeyStroke::parse("Space").unwrap().code,
        StrokeCode::Char(' ')
    );
}

#[test]
fn stroke_from_keycode_shift_letter() {
    let s = stroke_from_keycode(KeyCode::Char('J')).unwrap();
    assert!(s.shift);
    assert_eq!(s.code, StrokeCode::Char('j'));
}

#[test]
fn builtin_defaults_roundtrip_critical() {
    let store = KeymapStore::builtin();
    assert!(store.matches_code(ActionId::LogListMoveDown, KeyCode::Char('j')));
    assert!(store.matches_code(ActionId::LogListJumpDown, KeyCode::Char('J')));
    assert!(store.matches_code(ActionId::LogListLeader, KeyCode::Char(' ')));
    assert!(store.matches_code(ActionId::LogListExcludeChip, KeyCode::Char('C')));
    assert!(store.matches_code(ActionId::LeaderManage, KeyCode::Char(' ')));
    assert!(store.matches_code(ActionId::BookmarkAdd, KeyCode::Char('a')));
    assert!(store.matches_event(
        ActionId::LeaderPresetSave,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
    ));
    assert!(store.matches_event(
        ActionId::LeaderPresetOpen,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
    ));
    assert!(store.matches_event(
        ActionId::OpenFile,
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
    ));
    assert!(store.matches_event(
        ActionId::OpenStream,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
    ));
    assert!(!store.matches_event(
        ActionId::OpenFile,
        KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        ),
    ));
    assert!(!store.matches_code(ActionId::OpenFile, KeyCode::Char('f')));
    assert!(!store.matches_code(ActionId::OpenStream, KeyCode::Char('s')));
    assert!(!store.matches_code(ActionId::LeaderPresetSave, KeyCode::Char('w')));
    assert!(!store.matches_code(ActionId::LeaderPresetOpen, KeyCode::Char('o')));
    assert_eq!(
        store.display(ActionId::LogListClearLive).as_deref(),
        Some("C-l")
    );
    assert!(store.matches_event(
        ActionId::GlobalCommandPalette,
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
    ));
    assert!(store.matches_code(ActionId::HistPanelJumpDown, KeyCode::Char('J')));
    assert!(store.matches_code(ActionId::HistPanelJumpUp, KeyCode::Char('K')));
    assert!(store.matches_code(ActionId::HistPanelJumpTop, KeyCode::Char('g')));
    assert!(store.matches_code(ActionId::HistPanelJumpBottom, KeyCode::Char('G')));
    assert!(store.matches_code(ActionId::HistPanelZoomOut, KeyCode::Tab));
    assert!(store.matches_code(ActionId::HistPanelZoomIn, KeyCode::BackTab));
    assert!(!store.matches_code(ActionId::HistPanelZoomOut, KeyCode::Char('z')));
    assert!(!store.matches_code(ActionId::HistPanelZoomIn, KeyCode::Char('Z')));
    assert!(!store.matches_code(ActionId::GlobalCommandPalette, KeyCode::Char('p')));
    assert!(!store.matches_code(ActionId::GlobalCommandPalette, KeyCode::Char(':')));
    assert!(store.matches_code(ActionId::PaletteSubmit, KeyCode::Enter));
    assert!(store.matches_code(ActionId::PaletteClose, KeyCode::Esc));
}

#[test]
fn merge_override_and_unbind() {
    let store = merge_user_toml(
        r#"
[log_list]
move_down = "Down"
move_up = ""
"#,
    )
    .unwrap();
    assert!(store.matches_code(ActionId::LogListMoveDown, KeyCode::Down));
    assert!(!store.matches_code(ActionId::LogListMoveDown, KeyCode::Char('j')));
    assert!(store.binding(ActionId::LogListMoveUp).is_none());
    // untouched default
    assert!(store.matches_code(ActionId::LogListJumpTop, KeyCode::Char('g')));
}

#[test]
fn unknown_action_warns_but_applies_rest() {
    let store = merge_user_toml(
        r#"
[log_list]
move_down = "Down"
no_such = "x"
"#,
    )
    .unwrap();
    assert!(store.matches_code(ActionId::LogListMoveDown, KeyCode::Down));
    assert!(store.warnings.iter().any(|w| w.contains("no_such")));
}

#[test]
fn duplicate_binding_fails_hard() {
    let err = merge_user_toml(
        r#"
[log_list]
move_down = "k"
"#,
    )
    .unwrap_err();
    assert!(err.contains("duplicate") || err.contains("prefix"), "{err}");
}

#[test]
fn serialize_contains_sections() {
    let text = serialize_default_toml();
    assert!(text.contains("[log_list]"));
    assert!(text.contains("move_down = \"j\""));
    assert!(text.contains("[leader]"));
    assert!(text.contains("preset_save = \"C-s\""));
    assert!(text.contains("preset_open = \"C-o\""));
    assert!(text.contains("open_file = \"C-f\""));
    assert!(text.contains("open_stream = \"C-g\""));
    assert!(!text.contains("[open]"));
    assert!(!text.contains("open = \"o\""));
    assert!(text.contains("[command_palette]"));
    assert!(text.contains("command_palette = \"C-p\""));
    assert!(text.contains("highlight_new = \"/\""));
    assert!(text.contains("highlight_add = \"\""));
    assert!(text.contains("clear_all_rules = \"C-k\""));
    assert!(text.contains("submit = \"Enter\""));
    assert!(text.contains("[help]"));
    assert!(text.contains("back = \"h\""));
    assert!(text.contains("back_alt = \"Backspace\""));
    assert!(text.contains("search = \"/\""));
    assert!(text.contains("search_next = \"n\""));
    assert!(text.contains("search_prev = \"S-n\""));
    assert!(text.contains("submit = \"Enter\""));
}

#[test]
fn default_config_toml_documents_restore_session() {
    let text = serialize_default_config_toml();
    assert!(text.contains("restore_session = true"));
    let parsed: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(parsed["restore_session"].as_bool(), Some(true));
}

#[test]
fn clear_session_meta_is_unbound_file_only_palette_command() {
    let meta = ActionId::ClearSession.meta();
    assert_eq!(meta.context, KeyContext::Global);
    assert_eq!(meta.toml_key, "clear_session");
    assert!(meta.default.strokes.is_empty());
    assert_eq!(meta.capabilities, &[Capability::FileOnly]);
    assert!(meta.in_palette);
    assert_eq!(meta.palette_title, "Clear Session");
    assert_eq!(
        action_by_toml(KeyContext::Global, "clear_session"),
        Some(ActionId::ClearSession)
    );
}

#[test]
fn default_config_toml_documents_theme_names() {
    let text = serialize_default_config_toml();
    assert!(text.contains("theme = \"default\""));
    for name in [
        "onedark",
        "dracula",
        "everforest",
        "tokyo-night",
        "catppuccin-mocha",
        "gruvbox-dark",
        "nord",
        "kanagawa",
    ] {
        assert!(text.contains(name), "missing {name}");
    }
    let parsed: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(parsed["theme"].as_str(), Some("default"));
    assert!(text.contains("C-f"), "Open-file comments must use C-f");
    assert!(!text.contains("C-S-o"), "must not document Ctrl-Shift open");
    assert!(
        !text.contains(" / of"),
        "must not document retired of chord"
    );
}

#[test]
fn init_writes_and_skips() {
    let dir = tempfile_dir();
    let msgs = init_config_dir(&dir, false).unwrap();
    assert!(msgs.iter().any(|m| m.contains("keymap.toml")));
    assert!(dir.join("config.toml").is_file());
    assert!(dir.join("keymap.toml").is_file());
    let keymap = fs::read_to_string(dir.join("keymap.toml")).unwrap();
    assert!(keymap.contains("[command_palette]"));
    assert!(keymap.contains("command_palette = \"C-p\""));
    assert!(keymap.contains("[help]"));
    assert!(keymap.contains("search = \"/\""));
    assert!(keymap.contains("back = \"h\""));
    assert!(!dir.join("theme.toml").exists());
    let msgs2 = init_config_dir(&dir, false).unwrap();
    assert!(msgs2.iter().all(|m| m.starts_with("skip")));
}

fn tempfile_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("alnav-keymap-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn file_only_capability() {
    let meta = ActionId::LogListTime.meta();
    assert!(meta.allowed(true));
    assert!(!meta.allowed(false));
}

#[test]
fn registry_toml_keys_round_trip_and_are_unique_per_context() {
    let mut seen = std::collections::HashSet::new();
    for &id in ActionId::ALL {
        assert_eq!(id.meta().id, id);
        let (ctx, key) = (id.context(), id.toml_key());
        assert!(
            seen.insert((ctx, key)),
            "duplicate [{}] {key}",
            ctx.as_toml()
        );
        assert_eq!(
            action_by_toml(ctx, key),
            Some(id),
            "[{}] {key}",
            ctx.as_toml()
        );
    }
}
