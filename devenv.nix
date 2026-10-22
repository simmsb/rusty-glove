{ pkgs, lib, config, inputs, ... }:

let
  dfu_ble = pkgs.rustPlatform.buildRustPackage {
    pname = "ble-dfu";
    version = "0.1.0";
    src = inputs.ble-dfu;
    cargoLock = {
      lockFile = "${inputs.ble-dfu}/Cargo.lock";
      allowBuiltinFetchGit = true;
    };
    nativeBuildInputs = lib.optionals pkgs.stdenv.isLinux [ pkgs.pkg-config ];
    buildInputs = lib.optionals pkgs.stdenv.isLinux [ pkgs.dbus ];
  };
in
{
  packages = with pkgs; [
    flip-link
    cargo-show-asm
    cargo-bloat
    gcc-arm-embedded
    keylayout_lang
    dfu_ble
    python3
    keymap-drawer
  ];

  languages.rust = {
    enable = true;
    channel = "nightly";
    targets = [ "thumbv7em-none-eabihf" ];
    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "rust-src" "llvm-tools" ];
  };
}
