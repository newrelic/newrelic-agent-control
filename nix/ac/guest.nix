# The Linux system packages and VM tests are built for and run on: the host's own on Linux, and
# on Darwin the aarch64 Linux of the Linux builder and of the VMs run under HVF.
{ lib, ... }:
let
  inherit (lib) mkOption types;
in
{
  perSystem =
    {
      config,
      pkgs,
      system,
      ...
    }:
    {
      options.ac.guest = {
        system = mkOption {
          type = types.str;
          readOnly = true;
          description = "Nix system of the Linux guests, e.g. `x86_64-linux`.";
        };

        arch = mkOption {
          type = types.str;
          readOnly = true;
          description = "CPU architecture of `guest.system`: `x86_64` or `aarch64`.";
        };
      };

      config.ac.guest = {
        system = if pkgs.stdenv.hostPlatform.isDarwin then "aarch64-linux" else system;
        arch = lib.head (lib.splitString "-" config.ac.guest.system);
      };
    };
}
