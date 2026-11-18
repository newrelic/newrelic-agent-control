# The distributions the VM tests run on, shared by distros.nix and process-scoping.nix. Not a
# module.
# `share` is how the package and test files reach the guest: a 9p shared directory, or HTTP
# served by the test driver for kernels built without 9p (RHEL and its clones).
{
  ubuntu-20_04 = {
    image = "ubuntu";
    version = "20_04";
    format = "deb";
    share = "9p";
    layout = "hybrid";
  };
  ubuntu-22_04 = {
    image = "ubuntu";
    version = "22_04";
    format = "deb";
    share = "9p";
    layout = "v2";
  };
  rocky-9_6 = {
    image = "rocky";
    version = "9_6";
    format = "rpm";
    share = "http";
    layout = "v2";
  };
  # RHEL 8 compatible, cgroup v1. Its aarch64 image does not boot on Apple silicon (64 KiB pages).
  rocky-8_9 = {
    image = "rocky";
    version = "8_9";
    format = "rpm";
    share = "http";
    layout = "v1";
    systems = [ "x86_64-linux" ];
  };
}
