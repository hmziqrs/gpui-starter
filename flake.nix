{
  description = "gpui-starter flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        packages.default = let
          # Read name/version/description/repository straight out of Cargo.toml
          # so this flake stays in sync with the crate metadata.
          inherit ((pkgs.lib.importTOML ./Cargo.toml).package) name version description repository;
        in
          pkgs.rustPlatform.buildRustPackage {
            pname = name;
            inherit version;

            src = self;
            # allowBuiltinFetchGit is load-bearing: the `gpui` workspace dep is a
            # git source (zed-industries/zed), and cargo2nix's builtin fetcher
            # resolves it inside the Nix sandbox without a separate codegen step.
            cargoLock = {
              lockFile = ./Cargo.lock;
              allowBuiltinFetchGit = true;
            };

            meta = with pkgs.lib; {
              mainProgram = name;
              inherit description;
              homepage = repository;
              license = licenses.mit;
              platforms = platforms.linux;
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              openssl
              libxkbcommon
              libxcb
              wayland
              freetype
              fontconfig
            ];

            # GPUI renders via Vulkan on Linux and talks to the Wayland display
            # server; both of those loaders live in the Nix store and must be
            # added to the binary's RUNPATH so it can dlopen them at runtime.
            postFixup = with pkgs; ''
              patchelf --add-rpath ${vulkan-loader}/lib $out/bin/${name}
              patchelf --add-rpath ${wayland}/lib $out/bin/${name}
            '';
          };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            cargo
            rustc
            rust-analyzer
            rustfmt
            clippy
            pkg-config
          ];

          buildInputs = with pkgs; [
            openssl
            libxkbcommon
            libxcb
            wayland
            freetype
            fontconfig
          ];

          env = {
            # Make the Wayland / xkb / Vulkan / xcb shared objects discoverable
            # to anything that probes LD_LIBRARY_PATH (GPUI's render backends).
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
              wayland
              libxkbcommon
              vulkan-loader
              libxcb
            ];
          };
        };
      }
    );
}
