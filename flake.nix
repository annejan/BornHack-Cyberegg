{
  description = "BornHack CyberÆgg Badge Firmware";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in
      {
        devShells.default =
          with pkgs;
          mkShell rec {
            packages = [
              gnumake
              # scripts/build_guide.py needs the typst version in guide/typst-version.
              # This is nixpkgs' typst, which may differ; the build then reports
              # the version it found and the one it needs (not pinned here).
              typst
              python3
            ];

            buildInputs = [
              udev
              dfu-util
              probe-rs-tools
              gcc-arm-embedded
              pkg-config
              libudev-zero
              libclang
              flip-link

              (rust-bin.beta.latest.default.override {
                extensions = [ "rust-src" ];
                targets = [ "thumbv7em-none-eabihf" ];
              })
            ];

            shellHook = ''
              export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${toString (pkgs.lib.makeLibraryPath buildInputs)}";
            '';
          };
      }
    );
}
