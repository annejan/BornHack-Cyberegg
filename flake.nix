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
