
{
  description = "Rust development environment for Riff";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          rustc
          cargo
          rust-analyzer
          rustfmt
          clippy
          pkg-config
          gcc
          git
        ];

        RUST_BACKTRACE = "1";

        shellHook = ''
          echo "Riff Rust development environment"
          rustc --version
          cargo --version
        '';
      };
    };
}
