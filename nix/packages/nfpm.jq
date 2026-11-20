# Maps the `nfpms` entry of .goreleaser.yml to an nfpm config.
# File modes are octal strings in the goreleaser YAML and plain integers (read as decimal) in the
# nfpm config. Arguments: $arch (nfpm), $version, $bins (store path of the musl binaries).
def mode: explode | map(. - 48) | reduce .[] as $d (0; . * 8 + $d);
def bin($name): {
  src: "\($bins)/bin/\($name)",
  dst: "/usr/bin/\($name)",
  file_info: { mode: ("0755" | mode) }
};
.nfpms[0] as $n | {
  name: $n.package_name,
  arch: $arch,
  version: $version,
  maintainer: $n.maintainer,
  description: $n.description,
  license: $n.license,
  contents: (
    ($n.contents | map(if .file_info.mode then .file_info.mode |= mode else . end))
    + [bin("newrelic-agent-control"), bin("newrelic-agent-control-cli")]
  ),
  scripts: $n.scripts,
  provides: $n.provides,
  conflicts: $n.conflicts,
  replaces: $n.replaces,
  recommends: $n.recommends,
  overrides: $n.overrides
}
