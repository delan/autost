{
  pkgs ? import <nixpkgs> { },
}:

pkgs.mkShell {
  name = "autost-dev-shell";

  packages = with pkgs; [
    cargo
    rustc
    rustfmt
    clippy
    mold
    # FIXME: error: hash mismatch in fixed-output derivation '/nix/store/cdgc574mcx9x6bpzrpmkxz2ra3bzvkmd-source.drv':
    #                likely URL: https://github.com/rust-lang/rust-analyzer/archive/2025-08-11.tar.gz
    # rust-analyzer

    nixd
    nixfmt-rfc-style
  ];
}
