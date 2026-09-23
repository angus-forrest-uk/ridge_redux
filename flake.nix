{
  description = "Python reference env for parity fixtures, plus the C toolchain cargo needs to build";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAllSystems (pkgs: {
        # `uv` manages the numpy/scipy/scikit-image environment declared in
        # scripts/pyproject.toml; python312 is provided so uv has an
        # interpreter with wheels available (no build-from-source).
        # gcc provides the `cc` linker cargo needs (build scripts, final
        # binaries) — on NixOS there is no system-wide cc.
        default = pkgs.mkShell {
          packages = [ pkgs.uv pkgs.python312 pkgs.gcc ];
        };
      });
    };
}
