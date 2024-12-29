{
  pkgs ? import <nixpkgs> { },
}:

pkgs.mkShell {
  name = "autost-dev-shell";

  packages = with pkgs; [
    cargo
    rustc
    rustfmt
    rust-analyzer

    nixd
    nixfmt-rfc-style
  ];
}
