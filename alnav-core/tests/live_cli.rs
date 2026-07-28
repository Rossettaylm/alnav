use alnav::Cli;
use clap::Parser;

#[test]
fn adb_source_is_accepted() {
    let cli = Cli::try_parse_from(["alnav", "--adb"]).expect("--adb should be accepted");
    assert!(cli.adb);
    assert!(!cli.hdc);
}

#[test]
fn adb_device_is_accepted() {
    let cli = Cli::try_parse_from(["alnav", "--adb", "--device", "SERIAL"])
        .expect("--adb --device should be accepted");
    assert!(cli.adb);
    assert_eq!(cli.device.as_deref(), Some("SERIAL"));
}
