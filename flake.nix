{
  description = "A GitHub Action for publishing Nix flakes to FlakeHub";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/DeterminateSystems/secure/0";
    crane.url = "https://flakehub.com/f/ipetkov/crane/0.20.1";
    fenix = {
      url = "https://flakehub.com/f/nix-community/fenix/0.1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, ... }@inputs:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forAllSystems = forSystems supportedSystems;
      forDockerSystems = forSystems [ "x86_64-linux" ];

      forSystems =
        s: f:
        inputs.nixpkgs.lib.genAttrs s (
          system:
          f rec {
            inherit system;
            pkgs = import inputs.nixpkgs {
              inherit system;
              overlays = [ inputs.self.overlays.default ];
            };
          }
        );
    in
    {
      overlays.default = final: prev: rec {
        system = final.stdenv.hostPlatform.system;

        rustToolchain =
          with inputs.fenix.packages.${system};
          combine (
            [
              stable.clippy
              stable.rustc
              stable.cargo
              stable.rustfmt
              stable.rust-src
            ]
            ++ final.lib.optionals (system == "x86_64-linux") [
              targets.x86_64-unknown-linux-musl.stable.rust-std
            ]
            ++ final.lib.optionals (system == "aarch64-linux") [
              targets.aarch64-unknown-linux-musl.stable.rust-std
            ]
          );

        craneLib = (inputs.crane.mkLib final).overrideToolchain rustToolchain;

        flakehub-push = inputs.self.packages.${final.stdenv.system}.flakehub-push;
      };

      packages = forAllSystems (
        { system, pkgs, ... }:
        rec {
          default = flakehub-push;

          flakehub-push = pkgs.craneLib.buildPackage (
            {
              pname = "flakehub-push";
              version = "0.1.0";
              src = pkgs.craneLib.path (
                builtins.path {
                  name = "determinate-nixd-source";
                  path = inputs.self;
                  filter = (
                    path: type:
                    baseNameOf path != "ts"
                    && baseNameOf path != "dist"
                    && baseNameOf path != ".github"
                    && path != "flake.nix"
                  );
                }
              );

              buildInputs = pkgs.lib.optionals (pkgs.stdenv.isDarwin) (
                with pkgs;
                [
                  libiconv
                ]
              );
            }
            // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
              CARGO_BUILD_TARGET =
                {
                  "x86_64-linux" = "x86_64-unknown-linux-musl";
                  "aarch64-linux" = "aarch64-unknown-linux-musl";
                }
                ."${pkgs.stdenv.system}" or null;
              CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
            }
          );
        }
      );

      devShells = forAllSystems (
        { system, pkgs, ... }:
        {
          default = pkgs.mkShell {
            name = "dev";
            buildInputs =
              with pkgs;
              [
                rustfmt
                cargo-outdated
                cargo-watch
                rust-analyzer
                rustc
                cargo

                nodejs_latest
                bacon

                self.formatter.${system}
              ]
              ++ inputs.self.packages.${system}.flakehub-push.buildInputs;

            nativeBuildInputs =
              with pkgs;
              [
              ]
              ++ inputs.self.packages.${system}.flakehub-push.nativeBuildInputs;
          };
        }
      );

      formatter = forAllSystems ({ pkgs, ... }: pkgs.nixfmt-rfc-style);

      dockerImages = forDockerSystems (
        { system, pkgs, ... }:
        {
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
        }
      );
    };
}
