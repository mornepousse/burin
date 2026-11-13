{
  description = "Burin: draw, model and cut for CNC carving";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # What the window needs when it opens: the program finds these at run time.
      runtimeLibs = pkgs: with pkgs; [
        libGL
        libxkbcommon
        wayland
        libx11
        libxcursor
        libxi
        libxrandr
        vulkan-loader
      ];
    in
    {
      packages = forAll (pkgs:
        let
          lib = pkgs.lib;
          version = (lib.importTOML ./Cargo.toml).package.version;
        in
        rec {
          burin = pkgs.rustPlatform.buildRustPackage {
            pname = "burin";
            inherit version;
            # Only what the program is built from.
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./build.rs ./src ./assets ./release ];
            };
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [ pkgs.pkg-config pkgs.makeWrapper ];
            buildInputs = runtimeLibs pkgs;

            # Where "Report a problem" and the new-version check point to.
            env.BURIN_ISSUES_URL = "https://github.com/mornepousse/burin/issues/new";

            # The tests drive the whole interface and read local jobs; they run in development.
            doCheck = false;

            postInstall = ''
              install -Dm644 release/burin.desktop $out/share/applications/burin.desktop
              install -Dm644 release/burin.png $out/share/icons/hicolor/256x256/apps/burin.png
              install -Dm644 release/burin.svg $out/share/icons/hicolor/scalable/apps/burin.svg
            '';

            postFixup = ''
              wrapProgram $out/bin/burin --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath (runtimeLibs pkgs)}
            '';

            meta = {
              description = "Draw, model and cut for CNC carving";
              homepage = "https://burin.cc";
              license = lib.licenses.gpl3Plus;
              mainProgram = "burin";
              platforms = lib.platforms.linux;
            };
          };
          default = burin;
        });

      apps = forAll (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.burin}/bin/burin";
          meta.description = "Run Burin";
        };
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer pkg-config ];
          buildInputs = runtimeLibs pkgs;
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (runtimeLibs pkgs);
        };
      });
    };
}
