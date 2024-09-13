{ pkgs ? import <nixpkgs> { } }:
with pkgs;
stdenvNoCC.mkDerivation {
  name = "dev-shell";
  nativeBuildInputs = [ cargo-edit cargo-readme cargo-watch rustup ];
  buildInputs = [ llvmPackages_latest.clang pkg-config openssl ];
}
