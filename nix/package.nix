{ lib, rustPlatform }:

let
  fs = lib.fileset;

  autostSources = fs.intersection (fs.gitTracked ../.) (
    fs.unions [
      ../Cargo.lock
      ../Cargo.toml
      ../autost.toml.example
      ../src
      ../static
      ../templates
    ]
  );
in
rustPlatform.buildRustPackage {
  pname = "autost";
  version = "1.4.0";

  src = fs.toSource {
    root = ../.;
    fileset = autostSources;
  };

  # don't forget to update this hash when Cargo.lock or ${version} changes!
  cargoHash = "sha256-0r0HoXF0jrrUyVdssGZeZTy6801HtT0a88rGoup8n9o=";

  # tell rust that the version should be “x.y.z-nix”
  # FIXME: nix package does not have access to git
  # <https://github.com/NixOS/nix/issues/7201>
  AUTOST_IS_NIX_BUILD = 1;

  meta = {
    description = "cohost-compatible blog engine and feed reader";
    homepage = "https://github.com/delan/autost";
    downloadPage = "https://github.com/delan/autost/releases";
    changelog = "https://github.com/delan/autost/blob/main/CHANGELOG.md";
    license = lib.licenses.isc;
    mainProgram = "autost";
    platforms = lib.platforms.all;
  };
}
