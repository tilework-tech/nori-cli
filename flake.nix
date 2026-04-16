{
  description = "Development Nix flake for Nori Codex CLI";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems f;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          nori_rs = pkgs.callPackage ./nori-rs { };
        in
        {
          nori-rs = nori_rs;
          default = nori_rs;
        }
      );

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          # mold is Linux-only, use system linker on macOS
          linkerPkgs = if pkgs.stdenv.isLinux then [ pkgs.mold pkgs.clang ] else [ ];
        in
        {
          default = pkgs.mkShell {
            # Inherit build dependencies from package definition
            inputsFrom = [ (pkgs.callPackage ./nori-rs { }) ];

            # Additional dev tools (Rust toolchain managed by rustup)
            buildInputs = linkerPkgs ++ [
              pkgs.sccache
            ];

            shellHook = ''
              # OpenSSL library path for runtime
              export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath [ pkgs.openssl ]}:$LD_LIBRARY_PATH

              # Enable sccache
              export RUSTC_WRAPPER=${pkgs.sccache}/bin/sccache
              export SCCACHE_CACHE_SIZE=10G
            '';
          };
        }
      );
    };
}
