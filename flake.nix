{
  description = "COSMIC-EXT-QUAKE-TERMINAL";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
      };
    in
    {
      packages.${system} = rec {
        default = pkgs.callPackage (./default.nix) {
          self = self;
          lib = pkgs.lib;
        };
        cosmic-ext-quake-terminal = default;
      };

      devShells.${system} = {
        default = pkgs.mkShell {
          name = "cosmic-ext-quake-terminal-dev-shell";
          buildInputs = [ self.packages.${system}.cosmic-ext-quake-terminal ];
        };
      };
    };
}
