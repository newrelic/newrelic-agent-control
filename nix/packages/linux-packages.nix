# deb and rpm packages built with nfpm from the musl binaries.
# The package definition is read from the `nfpms` section of .goreleaser.yml (see nfpm.jq), so
# releases and these packages share the same file layout, maintainer scripts and relations.
{ self, ... }:
{
  perSystem =
    {
      config,
      pkgs,
      lib,
      ...
    }:
    let
      ac = config.ac;

      # The musl builds are Linux outputs; a Darwin host takes them from its Linux builder.
      musl = self.packages.${ac.guest.system};

      archs = {
        x86_64 = {
          nfpm = "amd64";
          binaries = musl.x86_64-linux-musl;
        };
        aarch64 = {
          nfpm = "arm64";
          binaries = musl.aarch64-linux-musl;
        };
      };

      packagingSrc = lib.fileset.toSource {
        root = ../../.;
        fileset = lib.fileset.unions [
          ../../.goreleaser.yml
          ../../LICENSE.md
          ../../THIRD_PARTY_NOTICES.md
          (lib.fileset.difference ../../build/package ../../build/package/windows)
        ];
      };

      mkPackage =
        format: archName:
        let
          arch = archs.${archName};
        in
        pkgs.runCommand "newrelic-agent-control-${format}-${archName}"
          {
            nativeBuildInputs = [
              pkgs.jaq
              pkgs.nfpm
            ];
          }
          ''
            cp -r --no-preserve=mode ${packagingSrc}/. .
            jaq --from yaml --to json \
              --arg arch ${arch.nfpm} \
              --arg version ${ac.commonArgs.version} \
              --arg bins ${arch.binaries} \
              --from-file ${./nfpm.jq} .goreleaser.yml > nfpm.yaml
            mkdir $out
            nfpm package --config nfpm.yaml --packager ${format} --target $out/
          '';
    in
    {
      packages = lib.listToAttrs (
        map (p: lib.nameValuePair "${p.format}-${p.arch}" (mkPackage p.format p.arch)) (
          lib.cartesianProduct {
            format = [
              "deb"
              "rpm"
            ];
            arch = lib.attrNames archs;
          }
        )
      );
    };
}
