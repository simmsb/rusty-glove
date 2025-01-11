{
  description = "things";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
    };

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    parts.url = "github:hercules-ci/flake-parts";
    parts.inputs.nixpkgs-lib.follows = "nixpkgs";

    keylayout.url = "github:simmsb/keylayout";
    ble-dfu.url = "github:simmsb/rust-ble-dfu";
    elf2uf2.url = "github:simmsb/elf2uf2-rs";

    uf2.url = "github:microsoft/uf2";
    uf2.flake = false;

    keymap-drawer = {
      url = "github:caksoylar/keymap-drawer/a2a3b37c54ddb449428b4597b39c3c28b331a7da";
      flake = false;
    };
    poetry2nix = {
      url = "github:nix-community/poetry2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    bmp.url = "github:blackmagic-debug/blackmagic";
    bmp.flake = false;
  };

  outputs =
    inputs@{ self
    , nixpkgs
    , crane
    , fenix
    , parts
    , ...
    }:
    parts.lib.mkFlake { inherit inputs; } {
      systems = nixpkgs.lib.systems.flakeExposed;
      imports = [
      ];
      perSystem = { config, pkgs, system, lib, ... }:
        let
          poetry2nix = inputs.poetry2nix.lib.mkPoetry2Nix { inherit pkgs; };

          keymap-drawer = poetry2nix.mkPoetryApplication {
            name = "keymap-drawer";
            projectDir = inputs.keymap-drawer;
            preferWheels = true;
            meta = {
              mainProgram = "keymap";
            };
          };

          blackmagic_ =
            pkgs.stdenv.mkDerivation rec {
              pname = "blackmagic";
              version = "git";
              firmwareVersion = "v${version}";

              src = inputs.bmp;

              nativeBuildInputs = with pkgs; [
                meson
                ninja
                gcc-arm-embedded
                pkg-config
                python3
              ];

              buildInputs = with pkgs; [
                hidapi
                libftdi1
                libusb1
              ];

              strictDeps = true;

              postPatch = ''
                # Fix scripts that generate headers:
                for f in $(find scripts libopencm3/scripts -type f); do
                  patchShebangs "$f"
                done
              '';

              installPhase = ''
                mkdir -p "$out/bin"
                cp blackmagic $out/bin
              '';

              enableParallelBuilding = true;
            };

          uf2conv = pkgs.writeScriptBin "uf2conv" ''
            ${pkgs.python3}/bin/python ${inputs.uf2}/utils/uf2conv.py $*
          '';
          arm-toolchain-plain = fenix.packages.${system}.fromToolchainFile {
            file = ./rust-toolchain.toml;
            sha256 = "sha256-7m3k8755ugSBaNOk1dvdVBFJ3zMsNln24GDlD7lHolk=";
          };
          native-toolchain = (fenix.packages.${system}.complete.withComponents [
            "cargo"
            "clippy"
            # "rust-src"
            # "rustc"
            # "rustfmt"
          ]);
          arm-toolchain = pkgs.runCommand "turbowaker-rust" { } ''
            echo "test $out ${arm-toolchain-plain}"
            cp -RL ${arm-toolchain-plain} $out
            chmod -R +rwx $out

            echo "doing patch"

            patch $out/lib/rustlib/src/rust/library/core/Cargo.toml ${./turbowaker/Cargo.toml.patch}
            patch $out/lib/rustlib/src/rust/library/core/src/task/wake.rs ${./turbowaker/wake.rs.patch}
          '';

          toolchain = fenix.packages.${system}.combine [ arm-toolchain native-toolchain ];
          craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

          src = craneLib.cleanCargoSource ./.;

          package = { target ? "thumbv7em-none-eabihf", args ? "", profile ? "release", defmt ? "off" }: craneLib.buildPackage {
            inherit src;

            cargoVendorDir = craneLib.vendorMultipleCargoDeps {
              inherit (craneLib.findCargoFiles src) cargoConfigs;
              cargoLockList = [
                ./Cargo.lock

                # Unfortunately this approach requires IFD (import-from-derivation)
                # otherwise Nix will refuse to read the Cargo.lock from our toolchain
                # (unless we build with `--impure`).
                #
                # Another way around this is to manually copy the rustlib `Cargo.lock`
                # to the repo and import it with `./path/to/rustlib/Cargo.lock` which
                # will avoid IFD entirely but will require manually keeping the file
                # up to date!
                "${toolchain}/lib/rustlib/src/rust/library/Cargo.lock"
              ];
            };

            cargoExtraArgs = "-Z build-std=core,panic_abort,alloc -Z build-std-features=optimize_for_size,panic_immediate_abort,core/turbowakers --target ${target} ${args}";
            CARGO_PROFILE = profile;
            DEFMT_LOG = defmt;
            pname = "rusty-glove";
            version = "0.1.0";

            strictDeps = true;
            doCheck = false;
            buildInputs = [
              # Add additional build inputs here
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
              # Additional darwin specific inputs can be set here
              pkgs.libiconv
            ];
          };
          elf = pkg: name: binname: pkgs.runCommandLocal "mkelf" { } ''
            mkdir -p $out
            cp ${pkg}/bin/${name} $out/${binname}.elf
          '';
          binary = pkg: name: pkgs.runCommandLocal "mkbinary" { buildInputs = [ pkgs.llvm ]; } ''
            mkdir -p $out
            llvm-objcopy -O binary ${pkg}/bin/${name} $out/${name}.bin
          '';
        in
        rec
        {
          _module.args.pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [
              inputs.keylayout.overlays.default
              inputs.ble-dfu.overlays.default
              inputs.elf2uf2.overlays.default
            ];
          };

          packages = builtins.mapAttrs
            (name: value: elf
              (package {
                args = "--bin binary -p rusty-glove --no-default-features --features '${value},default_unselected_side,reboot_on_panic'";
              }) "binary"
              name)
            { left = "side_left"; right = "side_right"; } // {
            default = pkgs.symlinkJoin {
              name = "combied";
              paths = [
                packages.left
                packages.right
              ];
            };

          };

          devShells.default = craneLib.devShell {
            packages = with pkgs; [
              libiconv
              just
              keylayout_lang
              uf2conv
              elf2uf2_rs
              keymap-drawer
              dfu_ble
              blackmagic_
              gcc-arm-embedded
            ];
          };
        };
    };
}
