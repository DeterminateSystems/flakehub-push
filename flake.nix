{
  description = "A https://nxfr.com/ pusher.";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];

      forAllSystems = forSystems supportedSystems;
      forDockerSystems = forSystems [ "x86_64-linux" ];

      forSystems = s: f: inputs.nixpkgs.lib.genAttrs s (system: f rec {
        inherit system;
        pkgs = import inputs.nixpkgs {
          inherit system;
          overlays = [
            inputs.self.overlays.default
            inputs.rust-overlay.overlays.default
          ];
        };
        lib = pkgs.lib;
      });
    in
    {
      overlays.default = final: prev: {
        nxfr-push = inputs.self.packages.${final.stdenv.system}.nxfr-push;
      };


      packages = forAllSystems ({ system, pkgs, lib, ... }:
        let
          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            targets = [ "x86_64-unknown-linux-musl" ];
          };
        in
        rec {
          default = nxfr-push;

          nxfr-push = craneLib.buildPackage {
            pname = "nxfr-push";
            version = "0.0.0";
            src = craneLib.path ./.;

            CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
            CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
          };
        });

      devShells = forAllSystems ({ system, pkgs, ... }: {
        default = pkgs.mkShell {
          name = "dev";
          buildInputs = with pkgs; [
            nixpkgs-fmt
            rustfmt
            cargo-outdated
            cargo-watch
            rust-analyzer
            rustc
            cargo
          ]
          ++ inputs.self.packages.${system}.nxfr-push.buildInputs;

          nativeBuildInputs = with pkgs; [
          ]
          ++ inputs.self.packages.${system}.nxfr-push.nativeBuildInputs;
        };
      });


      dockerImages = forDockerSystems ({ system, pkgs, ... }: {
        default = pkgs.dockerTools.buildLayeredImage {
          name = pkgs.nxfr-push.name;
          contents = [ pkgs.cacert ];
          config = {
            #ExposedPorts."8080/tcp" = { };
            Cmd = [ "${pkgs.nxfr-push}/bin/nxfr-push" ];
            Env = [
              "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            ];
          };
        };
      });
    };
}
