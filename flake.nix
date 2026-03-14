{
  description = "GPUI - GPU-accelerated UI framework extracted from Zed";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" ];
        };

        # Common dependencies for all platforms
        commonBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
          cmake
          perl
          openssl
        ];

        # Linux-specific dependencies
        linuxBuildInputs = with pkgs; [
          # Wayland
          wayland
          wayland-protocols
          libxkbcommon

          # X11
          xorg.libX11
          xorg.libXcursor
          xorg.libXrandr
          xorg.libXi
          xorg.libxcb
          xorg.xcbutilwm
          xorg.xcbutilimage
          xorg.xcbutilkeysyms
          xorg.xcbutilrenderutil

          # Vulkan (wgpu backend on Linux)
          vulkan-loader
          vulkan-headers
          vulkan-tools
          vulkan-validation-layers

          # Other
          libGL
          fontconfig
          freetype
          gtk3
          glib
          libgit2
          zlib
          zstd
          sqlite
          libsecret
          dbus
          alsa-lib
        ];

        # macOS-specific dependencies
        darwinBuildInputs = with pkgs; [
          apple-sdk_15
          libiconv
        ];

        platformBuildInputs =
          if pkgs.stdenv.isLinux then linuxBuildInputs
          else if pkgs.stdenv.isDarwin then darwinBuildInputs
          else [];

        # Environment variables needed for linking
        linuxEnvVars = pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [
            vulkan-loader
            libGL
            wayland
            libxkbcommon
            xorg.libX11
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            fontconfig
            freetype
          ]);
          VULKAN_SDK = "${pkgs.vulkan-headers}";
          VK_LAYER_PATH = "${pkgs.vulkan-validation-layers}/share/vulkan/explicit_layer.d";
        };

      in
      {
        devShells.default = pkgs.mkShell (
          {
            buildInputs = commonBuildInputs ++ platformBuildInputs;

            RUST_BACKTRACE = 1;
          } // linuxEnvVars
          // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
            MACOSX_DEPLOYMENT_TARGET = "10.15";
          }
        );
      }
    );
}
