# Linux distribution packages (DEB/RPM)
{ pkgs, rust, binaryPackages }:

let
  inherit (rust) agentControlCargoToml;

  # Function to create deb/rpm packages from a binary package
  mkLinuxPackage =
    {
      binaryPackage,
      arch, # "x86_64" or "aarch64"
      format, # "deb" or "rpm"
    }:
    let
      # Map architecture names
      debArch = if arch == "x86_64" then "amd64" else "arm64";
      rpmArch = if arch == "x86_64" then "x86_64" else "aarch64";
      pkgArch = if format == "deb" then debArch else rpmArch;

      version = agentControlCargoToml.package.version;
      packageName = "newrelic-agent-control";
    in
    pkgs.stdenv.mkDerivation {
      pname = "${packageName}-${format}-${arch}";
      inherit version;

      src = ../.;

      nativeBuildInputs = [ pkgs.nfpm ];

      buildPhase = ''
        # Create package structure
        mkdir -p package/usr/bin
        mkdir -p package/etc/newrelic-agent-control/local-data/agent-control
        mkdir -p package/lib/systemd/system
        mkdir -p package/usr/share/doc/newrelic/newrelic-agent-control
        mkdir -p package/var/lib/newrelic-agent-control/filesystem

        # Copy binaries from the binary package
        cp ${binaryPackage}/bin/newrelic-agent-control package/usr/bin/
        cp ${binaryPackage}/bin/newrelic-agent-control-cli package/usr/bin/
        chmod +x package/usr/bin/newrelic-agent-control
        chmod +x package/usr/bin/newrelic-agent-control-cli

        # Copy configuration and service files
        cp ${../build/package/config.yaml} package/etc/newrelic-agent-control/local-data/agent-control/local_config.yaml
        cp ${../build/package/newrelic-agent-control.service} package/lib/systemd/system/
        cp ${../build/package/newrelic-agent-control.conf} package/etc/newrelic-agent-control/systemd-env.conf
        cp ${../LICENSE.md} package/usr/share/doc/newrelic/newrelic-agent-control/

        # Create nfpm configuration
        cat > nfpm.yaml <<EOF
        name: ${packageName}
        arch: ${pkgArch}
        platform: linux
        version: ${version}
        maintainer: "New Relic <caos-team@newrelic.com>"
        description: "New Relic Agent Control - newrelic-agent-control"
        license: "Apache 2.0"

        provides:
          - newrelic-infra (= 3.0.0)
          - nr-otel-collector (= 2.0.0)

        conflicts:
          - newrelic-infra
          - nr-otel-collector

        replaces:
          - newrelic-infra
          - nr-otel-collector

        recommends:
          - td-agent-bit
          - fluent-bit

        contents:
          - src: package/usr/bin/newrelic-agent-control
            dst: /usr/bin/newrelic-agent-control
            file_info:
              mode: 0755
          - src: package/usr/bin/newrelic-agent-control-cli
            dst: /usr/bin/newrelic-agent-control-cli
            file_info:
              mode: 0755
          - src: package/etc/newrelic-agent-control/local-data/agent-control/local_config.yaml
            dst: /etc/newrelic-agent-control/local-data/agent-control/local_config.yaml
            type: config
            file_info:
              mode: 0600
          - src: package/lib/systemd/system/newrelic-agent-control.service
            dst: /lib/systemd/system/newrelic-agent-control.service
          - src: package/etc/newrelic-agent-control/systemd-env.conf
            dst: /etc/newrelic-agent-control/systemd-env.conf
            type: config|noreplace
            file_info:
              mode: 0600
          - src: package/usr/share/doc/newrelic/newrelic-agent-control/LICENSE.md
            dst: /usr/share/doc/newrelic/newrelic-agent-control/LICENSE.md
          - dst: /etc/newrelic-agent-control
            type: dir
            file_info:
              mode: 0700
          - dst: /var/lib/newrelic-agent-control
            type: dir
            file_info:
              mode: 0700
          - dst: /var/lib/newrelic-agent-control/filesystem
            type: dir
            file_info:
              mode: 0700

        scripts:
          postinstall: ${../build/package/postinstall.sh}
          preremove: ${../build/package/preremove.sh}
          postremove: ${../build/package/postremove.sh}
        EOF

        # Build the package
        nfpm package --packager ${format} --target .
      '';

      installPhase = ''
        mkdir -p $out
        cp *.${format} $out/
      '';

      meta = {
        description = "New Relic Agent Control - ${format} package for ${arch}";
        license = pkgs.lib.licenses.asl20;
        platforms = [ "${arch}-linux" ];
      };
    };

  # Create all distribution packages
  packages = {
    # DEB packages
    deb-x86_64 = mkLinuxPackage {
      binaryPackage = binaryPackages.x86_64-linux-musl;
      arch = "x86_64";
      format = "deb";
    };

    deb-aarch64 = mkLinuxPackage {
      binaryPackage = binaryPackages.aarch64-linux-musl;
      arch = "aarch64";
      format = "deb";
    };

    # RPM packages
    rpm-x86_64 = mkLinuxPackage {
      binaryPackage = binaryPackages.x86_64-linux-musl;
      arch = "x86_64";
      format = "rpm";
    };

    rpm-aarch64 = mkLinuxPackage {
      binaryPackage = binaryPackages.aarch64-linux-musl;
      arch = "aarch64";
      format = "rpm";
    };

    # Convenience: build all Linux binaries
    all-linux-binaries = pkgs.symlinkJoin {
      name = "newrelic-agent-control-binaries-${agentControlCargoToml.package.version}";
      paths = [
        binaryPackages.x86_64-linux-musl
        binaryPackages.aarch64-linux-musl
      ];
    };

    # Convenience: build all DEB packages
    all-deb-packages = pkgs.symlinkJoin {
      name = "newrelic-agent-control-all-deb-${agentControlCargoToml.package.version}";
      paths = [
        packages.deb-x86_64
        packages.deb-aarch64
      ];
    };

    # Convenience: build all RPM packages
    all-rpm-packages = pkgs.symlinkJoin {
      name = "newrelic-agent-control-all-rpm-${agentControlCargoToml.package.version}";
      paths = [
        packages.rpm-x86_64
        packages.rpm-aarch64
      ];
    };

    # Convenience: build everything (binaries + all packages)
    all-linux-packages = pkgs.symlinkJoin {
      name = "newrelic-agent-control-all-${agentControlCargoToml.package.version}";
      paths = [
        binaryPackages.x86_64-linux-musl
        binaryPackages.aarch64-linux-musl
        packages.deb-x86_64
        packages.deb-aarch64
        packages.rpm-x86_64
        packages.rpm-aarch64
      ];
    };
  };
in
{
  inherit mkLinuxPackage packages;
}
