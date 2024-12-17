{
  description = "cohost-compatible blog engine and feed reader";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        appliedOverlay = self.overlays.default pkgs pkgs;
      in
      {
        packages = rec {
          inherit (appliedOverlay) autost;

          default = autost;
        };

        devShell = import ./shell.nix { inherit pkgs; };
      }
    )
    // {
      overlays.default = import ./nix/overlay.nix;
    };
}
