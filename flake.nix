{
  # Nix users get the *video* build by default: Nix builds `--features
  # video` against its own GStreamer and pins it as a runtime dependency
  # (plugin path wrapped in), so inline playback works with nothing else
  # installed — the strongest answer to the GStreamer distribution
  # problem on any channel.
  #
  #   nix run github:camjjack/preso -- talk.md
  #   nix profile install github:camjjack/preso
  #   nix develop   # shell with the C-level deps for `cargo build --features video`
  description = "preso — native markdown presentations (inline-video build)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      gstDeps = pkgs: with pkgs.gst_all_1; [
        gstreamer
        gst-plugins-base
        gst-plugins-good
        gst-plugins-bad
        gst-plugins-ugly
        gst-libav # H.264/AAC decoders for typical .mp4 clips
      ];

      # Libraries iced/winit/wgpu dlopen at run time on Linux (they are
      # not link-time dependencies, so they must be added to the rpath).
      linuxRuntimeLibs = pkgs: with pkgs; [
        vulkan-loader
        libGL
        wayland
        libxkbcommon
        xorg.libX11
        xorg.libXcursor
        xorg.libXi
        xorg.libXrandr
      ];

      # A function of pkgs so the overlay can build preso against the
      # *consumer's* nixpkgs (GStreamer and GUI libs then match their
      # system), while our own packages output uses the pinned input.
      mkPreso = pkgs:
        let
          inherit (pkgs) lib stdenv;
        in
        pkgs.rustPlatform.buildRustPackage {
            pname = "preso";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
            src = self;
            cargoLock.lockFile = ./Cargo.lock;

            # The app with inline video, plus the importer. The feature is
            # namespaced because two -p selections are built at once.
            cargoBuildFlags = [ "-p" "preso-app" "-p" "preso-convert" ];
            buildFeatures = [ "preso-app/video" ];

            # The full suite runs in repo CI; skipping it here avoids
            # rebuilding the workspace for tests under the fat-LTO profile.
            doCheck = false;

            nativeBuildInputs = with pkgs; [ pkg-config makeWrapper ];
            buildInputs = gstDeps pkgs
              ++ lib.optionals stdenv.isLinux (with pkgs; [
                libxkbcommon
                xorg.libxcb # clipboard_x11 links xcb at build time
              ]);

            postInstall = lib.optionalString stdenv.isLinux ''
              install -Dm644 assets/linux/preso.desktop \
                $out/share/applications/preso.desktop
              for n in 16 24 32 48 64 128 256 512; do
                install -Dm644 assets/icons/preso-$n.png \
                  "$out/share/icons/hicolor/$n"x"$n/apps/preso.png"
              done
            '';

            # GST_PLUGIN_SYSTEM_PATH_1_0 is assembled by the gstreamer
            # packages' setup hooks during the build; bake it into the
            # binary so plugin discovery works from any environment.
            postFixup = ''
              wrapProgram $out/bin/preso \
                --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "$GST_PLUGIN_SYSTEM_PATH_1_0"
            '' + lib.optionalString stdenv.isLinux ''
              patchelf --add-rpath ${lib.makeLibraryPath (linuxRuntimeLibs pkgs)} \
                $out/bin/.preso-wrapped
            '';

            meta = with lib; {
              description = "Native markdown presentations (inline-video build)";
              homepage = "https://github.com/camjjack/preso";
              license = with licenses; [ mit asl20 ];
              mainProgram = "preso";
            };
          };
    in
    {
      packages = forAll (pkgs: rec {
        preso = mkPreso pkgs;
        default = preso;
      });

      # For NixOS / home-manager configs: add the overlay and use
      # `pkgs.preso`. Built with the consumer's nixpkgs (see mkPreso).
      overlays.default = final: _prev: { preso = mkPreso final; };

      apps = forAll (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.system}.preso}/bin/preso";
        };
      });

      # `nix develop`: the C-level dependencies for building the video
      # feature with your own toolchain — pkg-config resolves the
      # GStreamer .pc files from here, no system GStreamer needed.
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = gstDeps pkgs
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux
              (with pkgs; [ libxkbcommon xorg.libxcb ] ++ linuxRuntimeLibs pkgs);
        };
      });
    };
}
