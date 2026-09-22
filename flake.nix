{
  description = "Python reference environment for generating ridge-core parity fixtures";

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
        default = pkgs.mkShell {
          packages = [ pkgs.uv pkgs.python312 ];
        };
      });
    };
}
