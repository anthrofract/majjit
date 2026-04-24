{
  description = "Majjit: A TUI to manipulate the Jujutsu DAG";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, nixpkgs, fenix }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;

      rustToolchain =
        system:
        fenix.packages.${system}.toolchainOf {
          channel = "1.91.1";
          sha256 = "sha256-SDu4snEWjuZU475PERvu+iO50Mi39KVjqCeJeNvpguU=";
        };

      mkMajjit =
        pkgs:
        let
          toolchain = rustToolchain pkgs.stdenv.hostPlatform.system;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain.cargo;
            rustc = toolchain.rustc;
          };
        in
        rustPlatform.buildRustPackage {
          pname = "majjit";
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = mkMajjit pkgs;
          majjit = mkMajjit pkgs;
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          toolchain = rustToolchain system;
        in
        {
          default = pkgs.mkShell {
            packages = [
              toolchain.toolchain
              pkgs.git
              pkgs.just
              pkgs.jujutsu
            ];
          };
        }
      );

      overlays.default = final: prev: {
        majjit = mkMajjit final;
      };
    };
}
