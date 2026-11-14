# Gives the guest the package and test files over HTTP, for kernels without 9p (RHEL and its
# clones): a server in the test driver, which the guest reaches at 10.0.2.2. SERVE_DIR and FORMAT
# are prepended by distros.nix.
import functools
import http.server
import threading

server = http.server.ThreadingHTTPServer(
    ("127.0.0.1", 0),
    functools.partial(http.server.SimpleHTTPRequestHandler, directory=SERVE_DIR),
)
threading.Thread(target=server.serve_forever, daemon=True).start()
base = f"http://10.0.2.2:{server.server_address[1]}"
vm.succeed("mkdir -p /mnt/pkg /mnt/config")
vm.succeed(f"curl -fsS -o /mnt/pkg/pkg.{FORMAT} {base}/pkg/pkg.{FORMAT}")
for name in ["sleeper-agent-type.yaml", "local_config.yaml"]:
    vm.succeed(f"curl -fsS -o /mnt/config/{name} {base}/config/{name}")
