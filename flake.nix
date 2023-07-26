{
  description = "A https://flakehub.com/ pusher.";

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
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

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
        cranePkgs = pkgs.callPackage ./crane.nix {
          inherit (inputs) crane;
          inherit supportedSystems;
          darwinFrameworks = with pkgs.darwin.apple_sdk.frameworks; [ Security ];
        };
        lib = pkgs.lib;
      });
    in
    {
      overlays.default = final: prev: {
        flakehub-push = inputs.self.packages.${final.stdenv.system}.flakehub-push;
      };

      packages = forAllSystems ({ cranePkgs, ... }: rec {
        flakehub-push = cranePkgs.package;
        default = flakehub-push;
      });

      devShells = forAllSystems ({ system, pkgs, cranePkgs, ... }: {
        default = pkgs.mkShell {
          name = "dev";
          buildInputs = with pkgs; [
            cranePkgs.rustNightly
            nixpkgs-fmt
            rustfmt
            cargo-outdated
            cargo-watch
          ]
          ++ inputs.self.packages.${system}.flakehub-push.buildInputs
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin (with pkgs.darwin.apple_sdk.frameworks; [ Security ]);

          nativeBuildInputs = with pkgs; [
          ]
          ++ inputs.self.packages.${system}.flakehub-push.nativeBuildInputs;
        };
      });

      dockerImages = forDockerSystems ({ system, pkgs, ... }: {
        default = pkgs.dockerTools.buildLayeredImage {
          name = pkgs.flakehub-push.name;
          contents = [ pkgs.cacert ];
          config = {
            #ExposedPorts."8080/tcp" = { };
            Cmd = [ "${pkgs.flakehub-push}/bin/flakehub-push" ];
            Env = [
              "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            ];
          };
        };
      });
    };
}
