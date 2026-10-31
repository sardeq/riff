{
  description = "Riff — a modern soundboard with a virtual microphone, hotkeys and local TTS";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      lib = pkgs.lib;

      # Loaded at runtime with dlopen by winit / glutin (Wayland, X11, OpenGL).
      runtimeLibs = with pkgs; [
        wayland
        libxkbcommon
        libGL
        (pkgs.libx11 or pkgs.xorg.libX11)
        (pkgs.libxcursor or pkgs.xorg.libXcursor)
        (pkgs.libxi or pkgs.xorg.libXi)
        (pkgs.libxrandr or pkgs.xorg.libXrandr)
      ];

      # Command-line tools Riff calls: pactl (virtual mic), espeak-ng + piper (TTS).
      runtimeTools = with pkgs; [
        pulseaudio # only for `pactl`; works with pipewire-pulse
        espeak-ng
        piper-tts
      ];
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
          libpulseaudio
        ] ++ runtimeTools;

        LD_LIBRARY_PATH = lib.makeLibraryPath (runtimeLibs ++ [ pkgs.libpulseaudio ]);
        RUST_BACKTRACE = "1";

        shellHook = ''
          echo "Riff Rust development environment"
          rustc --version
          cargo --version
        '';
      };

      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "riff";
        version = "0.1.0";
        src = lib.cleanSource ./.;
        cargoLock.lockFile = ./Cargo.lock;

        nativeBuildInputs = with pkgs; [ pkg-config makeWrapper ];
        buildInputs = with pkgs; [ libpulseaudio ];

        postInstall = ''
          wrapProgram $out/bin/riff \
            --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
            --prefix PATH : ${lib.makeBinPath runtimeTools}
          install -Dm644 assets/riff.desktop $out/share/applications/riff.desktop
          install -Dm644 assets/riff.svg $out/share/icons/hicolor/scalable/apps/riff.svg
        '';

        meta = {
          description = "Soundboard with a virtual microphone, global hotkeys, local TTS and Freesound";
          homepage = "https://github.com/sardeq/riff";
          license = lib.licenses.asl20;
          mainProgram = "riff";
          platforms = lib.platforms.linux;
        };
      };

      apps.${system}.default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/riff";
      };
    };
}
