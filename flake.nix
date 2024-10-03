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
    elf2uf2.url = "github:simmsb/elf2uf2-rs";

    uf2.url = "github:microsoft/uf2";
    uf2.flake = false;

    keymap-drawer = {
      url = "github:caksoylar/keymap-drawer";
      flake = false;
    };
    poetry2nix = {
      url = "github:nix-community/poetry2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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
          poetry2nix = inputs.poetry2nix.lib.mkPoetry2Nix {inherit pkgs;};

          keymap-drawer = poetry2nix.mkPoetryApplication {
            projectDir = inputs.keymap-drawer;
            preferWheels = true;
            meta = {
              mainProgram = "keymap";
            };
          };

          uf2conv = pkgs.writeScriptBin "uf2conv" ''
            ${pkgs.python3}/bin/python ${inputs.uf2}/utils/uf2conv.py $*
          '';
          arm-toolchain-plain = fenix.packages.${system}.fromToolchainFile {
            file = ./rust-toolchain.toml;
            sha256 = "sha256-UH3aTxjEdeXYn/uojGVTHrJzZRCc3ODd05EDFvHmtKE=";
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
          
          package = { target ? "thumbv7em-none-eabihf", args ? "", profile ? "release" }: craneLib.buildPackage {
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
                "${toolchain}/lib/rustlib/src/rust/Cargo.lock"
              ];
            };

            cargoExtraArgs = "-Z build-std=core,panic_abort,alloc -Z build-std-features=optimize_for_size,panic_immediate_abort,core/turbowakers --target ${target} ${args}";
            CARGO_PROFILE = profile;
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
          elf = pkg: name: pkgs.runCommandLocal "mkelf" { } ''
            mkdir -p $out
            cp ${pkg}/bin/${name} $out/${name}.elf
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
              inputs.elf2uf2.overlays.default
            ];
          };

          devShells.default = craneLib.devShell {
            packages = with pkgs; [ libiconv just keylayout_lang uf2conv elf2uf2_rs keymap-drawer ];
          };
        };
    };
}
