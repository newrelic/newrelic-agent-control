# Fluent Bit integration in the Infrastructure Agent

How the infra agent ships, locates, and runs Fluent Bit as its log forwarder — and
how this differs across Linux, Windows, and macOS/other. Covers both **runtime** and
**build/packaging**.

---

## TL;DR

- The agent runs Fluent Bit as a **supervised child process** (it does not embed it).
- **Linux**: FB engine comes from the **distro package manager** as a dependency of the
  agent package (installed to `/opt/fluent-bit/bin`). The agent only ships the NR output
  plugin (`out_newrelic.so`) + `parsers.conf`.
- **Windows**: the **entire FB engine is embedded in the MSI** (exe + dll + plugin),
  under the agent dir `newrelic-integrations\logging(-legacy)`.
- **macOS/other**: no log forwarding — FB paths resolve to `""` and the availability
  check fails.

---

## Runtime

Core code: `pkg/integrations/v4/`
- `supervisor_fb.go` — cross-platform supervisor: builds the command, availability
  check, temp config files, restart handling.
- `supervisor_fb_conf_linux.go` / `_windows.go` / `_others.go` — the only
  platform-specific part: **where the FB binary lives** and **which version to pick**.

Wiring at startup: `cmd/newrelic-infra/newrelic-infra.go:460` builds `fBSupervisorConfig`,
calls `IsLogForwarderAvailable()`, and if OK spawns `logSupervisor.Run()` in a goroutine.

### How the binary is launched (`buildFbExecutor`, supervisor_fb.go:181)
```
<fb-exe> -c <generated-tmp-config> -e <NR-output-plugin> -R <parsers.conf> [-R <external-parsers>] [-vv]
```
- Env: `NR_LICENSE_KEY_ENV_VAR=<license key>`.
- Config generated from `logging.d/*.yml` via `logs.CfgLoader.LoadAndFormat`, written to a
  temp file. Agent keeps at most 50 temp configs (`MaxNumberOfFbConfigTempFiles`).
- A file watcher on the logging config dir triggers restarts; so does the
  `fluent_bit_19_win` feature flag (`ffHandle.SetFBRestarter`).

### Availability gate (`IsLogForwarderAvailable`, supervisor_fb.go:107)
Requires **three** files to exist: FB executable, NR output plugin, `parsers.conf`.
Any missing → agent logs "Log forwarder is not available for this platform" and runs
without log forwarding.

### Path resolution precedence (`getFbPath`, supervisor_fb.go:125)
1. Explicit `fluent_bit_exe_path` config → wins.
2. Else `logging_bin_dir` config, if set.
3. Else platform default dir + platform default exe name (table below).

### Platform-specific binary location & version selection

| | Linux (`_linux.go`) | Windows (`_windows.go`) | macOS/other (`_others.go`) |
|---|---|---|---|
| FB 2.x/3.x path | `/opt/fluent-bit/bin/fluent-bit` | `<agentDir>/newrelic-integrations/logging/fluent-bit.exe` | `""` |
| FB 1.9 legacy path | `/opt/td-agent-bit/bin/td-agent-bit` | `<agentDir>/newrelic-integrations/logging-legacy/fluent-bit.exe` | `""` |
| Version chosen by | **What's on disk**: `onlyTdAgentInstalled()` picks legacy only if `td-agent-bit` exists AND `fluent-bit` does not; else modern | **Feature flag** `fluent_bit_19_win` (command channel): if exists & enabled → `logging-legacy` dir | n/a |
| NR output plugin | `out_newrelic.so` | `out_newrelic.dll` | n/a |
| Available? | Yes | Yes | **No** |

Note: on Linux the FF args to `defaultLoggingBinDir/defaultFluentBitExePath` are ignored
(`_ bool, _ bool`); selection is purely by which binary is present.

### Shared default config paths (`config.go:2306-2319` + `config_{linux,windows}.go`)
- `LoggingHomeDir`        = `<AgentDir>/newrelic-integrations/logging`
- `FluentBitParsersPath`  = `LoggingHomeDir/parsers.conf`
- `FluentBitNRLibPath`    = `LoggingHomeDir/out_newrelic.{so|dll}`
- `LoggingConfigsDir`     = `<ConfigDir>/logging.d` (Linux: `/etc/newrelic-infra/logging.d`)

Relevant feature flag: `FlagFluentBit19 = "fluent_bit_19_win"`
(`internal/agent/cmdchannel/fflag/ffhandler.go`).

---

## Configuration generation & log↔host correlation

The agent does not just launch Fluent Bit — it **generates the FB config on the fly** from
`logging.d/*.yml`, and while generating it, injects the host's identity as FB
`record_modifier` FILTERs. Those filters stamp every record with the entity GUID + hostname,
which is what lets the NR platform correlate logs with the host entity (and its metrics).

Core code: `pkg/integrations/v4/logs/`
- `loader.go` — `CfgLoader`: reads `logging.d/*.yml`, resolves identity/hostname, calls `NewFBConf`.
- `cfg.go` — YAML schema (`LogCfg`), `NewFBConf` (builds inputs/filters/output), metadata injection.
- `cfg_template.go` — Go `text/template` rendering `FBCfg` → `.conf` text.

### Data flow
```
logging.d/*.yml ──parse──▶ LogsCfg ──NewFBConf──▶ FBCfg ──Format(template)──▶ fluent-bit .conf
                                        ▲
              agent entity GUID + short hostname injected here
```

### 1. Config source — `logging.d/*.yml`
Loader reads every `.yml`/`.yaml` in `LoggingConfigsDir` (`/etc/newrelic-infra/logging.d/`),
parsing each into a `LogCfg` (`cfg.go:87`). Supported input types: `file` (tail), `systemd`,
`syslog`, `tcp`, `winlog`, `winevtlog`, plus per-entry `attributes:`, `pattern:`,
`max_line_kb`, multiline parsers, etc.
```yaml
logs:
  - name: app-log
    file: /var/log/app.log
    attributes:
      service: api
      env: production
```

### 2. Metadata injection — the key part (`NewFBConf`, cfg.go:280)
`NewFBConf(loggingCfgs, logFwdCfg, entityGUID, hostname)` builds the config. After turning each
log entry into an `[INPUT]`, it appends **one global `record_modifier` FILTER matching `*`**
(`cfg.go:334-342`), stamping **every** record regardless of source:
```go
fb.Filters = append(fb.Filters, FBCfgFilter{
    Name:  fbFilterTypeRecordModifier,
    Match: "*",
    Records: map[string]string{
        rAttEntityGUID: entityGUID,              // "entity.guid.INFRA"  → host entity GUID
        rAttPluginType: logRecordModifierSource, // "plugin.type"        = "nri-agent"
        rAttHostname:   hostname,                // "hostname"           → short hostname
    },
})
```
Constants at `cfg.go:70-75` / `29`. Additionally, **each input** gets its own `record_modifier`
FILTER (`newRecordModifierFilterForInput`, `cfg.go:690`) adding `fb.input` (e.g. `tail`,
`systemd`) plus the user's `attributes:` from YAML — with reserved names filtered out so users
cannot clobber the correlation fields (`cfg.go:784`).

### 3. Where identity/hostname come from (loader.go:71-78)
This is the "context" being passed in. On every (re)generation:
```go
agentGUID := l.agentIDFn().GUID              // blocks until the agent has registered & has an entity ID
_, shortHostName, err := l.hostnameResolver.Query()
c, err = NewFBConf(allFilesCfgs, &(l.config), agentGUID.String(), shortHostName)
```
Wired at startup (`cmd/newrelic-infra/newrelic-infra.go:~473`):
```go
logCfgLoader := logs.NewFolderLoader(logFwCfg, agt.Context.Identity, agt.Context.HostnameResolver())
```
`agt.Context.Identity` returns the live entity identity obtained at agent registration, so log
forwarding **waits** until the GUID exists — correlation is guaranteed possible. Identity/hostname
change notifiers trigger config regeneration + FB restart (see `NewFBSupervisor` wiring).

### 4. Rendering to text (`cfg_template.go`)
`FBCfg.Format()` runs the struct through a Go `text/template`. The record_modifier filters render
as (`cfg_template.go:93-96`):
```
[FILTER]
    Name   record_modifier
    Match  *
    Record entity.guid.INFRA <guid>
    Record hostname <host>
    Record plugin.type nri-agent
```

### 5. License key — via env, never in the config text
The template emits `licenseKey ${NR_LICENSE_KEY_ENV_VAR}` in `[OUTPUT]` (`cfg_template.go:116`).
The supervisor injects the actual key as an environment variable when spawning FB
(`supervisor_fb.go:~227`): `NR_LICENSE_KEY_ENV_VAR=<license>`.

### Metadata summary

| Metadata | Value | Where injected | Purpose |
|---|---|---|---|
| `entity.guid.INFRA` | host entity GUID | global `record_modifier` FILTER (`cfg.go:338`) | correlate logs with the infra host entity |
| `hostname` | short hostname | same filter (`cfg.go:340`) | host identification |
| `plugin.type` | `nri-agent` | same filter (`cfg.go:339`) | source tagging |
| `fb.input` + custom attrs | e.g. `tail` + user `attributes:` | per-input filter (`cfg.go:690`) | source + user labels |
| license key | from agent config | env var, not config text (`supervisor_fb.go`) | auth to Logs API |

---

## Fluent Bit configuration formats: classic vs. YAML — and what the agent actually emits

Fluent Bit supports two *native* config formats. It's easy to conflate these with the
agent's own YAML input schema, so this section draws the line clearly.

### Fluent Bit's two native formats

| | Classic format | Fluent Bit YAML format |
|---|---|---|
| Syntax | `[SECTION]` headers, indented `Key Value` entries, `@INCLUDE` — similar in spirit to INI | Standard YAML: `service`, `pipeline` (`inputs`/`filters`/`outputs`), `parsers`, `env`, `includes`, etc. |
| Introduced | Original format, present since the beginning | Experimental in v1.9, production-ready in v2.0, full feature parity (incl. `processors`) in v3.2 |
| Status today | Still fully supported, not deprecated | Fully supported, superset of classic since v3.2 |

### What the infra agent actually emits

The agent does **not** use Fluent Bit's YAML config mode. `NewFBConf` /
`FBCfg.Format()` (`cfg_template.go`) render **classic format** — see the generated
example above (`[INPUT]`, `[FILTER]`, `[OUTPUT]` blocks). This holds regardless of
platform, even though the bundled/expected FB engines (5.0.6 on Windows, similarly
modern on Linux) fully support the YAML mode — the classic output is a design choice
in the templating code, not a technical limitation.

### Don't confuse this with `logging.d/*.yml`

`logging.d/*.yml` (the `LogCfg` schema the agent reads as *input*, see
[Config source](#1-config-source--loggingdyml) above) is YAML — but it's the
**agent's own invented schema**, unrelated to Fluent Bit's YAML config format. The
agent parses this YAML, then translates it into Fluent Bit **classic** format on the
way out. In short:

```
logging.d/*.yml (agent's own YAML schema, input)
        │
        ▼  NewFBConf + Format()
fluent-bit .conf (Fluent Bit classic format, output — NOT FB's YAML mode)
```

---

## Build & packaging — this is where the OS split really lives

Versions pinned in `build/embed/fluent-bit.version`:
```
# OS, newrelic_plugin_version, fluent-bit
linux,3.6.0                 # only NR plugin pinned; FB engine handled by pkg manager
windows,3.6.0,5.0.6         # NR plugin 3.6.0 + FB 5.0.6 (bundled)
windows-legacy,1.19.1,1.9.3 # legacy path behind ff fluent_bit_19
```

### Linux — FB engine is an OS package dependency (NOT bundled)
`build/embed/fluent-bit.mk` downloads **only the NR output plugin** at build time:
```
.../newrelic-fluent-bit-output/releases/download/v3.6.0/out_newrelic-linux-<arch>-3.6.0.so
→ target/fluent-bit-plugin/<arch>/out_newrelic.so
```
goreleaser nfpm configs (`build/goreleaser/linux/*.yml`):
1. Ship into the agent package (under `/var/db/newrelic-infra/newrelic-integrations/logging/`):
   - `out_newrelic.so` (from build download)
   - `parsers.conf` (from `assets/examples/logging/parsers.conf`)
   - `*.yml.example` → `/etc/newrelic-infra/logging.d/`
2. Declare the FB engine as a **package dependency**, varying by distro:
   - Modern RPM/DEB (RHEL 9, Debian systemd, SLES 15.6): `recommends: [fluent-bit]` (weak).
   - Old pkg managers w/o weak deps (Amazon Linux 2, CentOS 7, SLES 12.5):
     `dependencies: [td-agent-bit, fluent-bit]` (**hard**).
   - CentOS 6: both commented out — **no FB dependency**.
   - `td-agent-bit` is the legacy 1.9 dep, marked "To be removed on removal of the ff fluent_bit_19".

Result: distro repo installs `fluent-bit` → `/opt/fluent-bit/bin/fluent-bit`, exactly what
`supervisor_fb_conf_linux.go` expects. Agent supplies plugin + parsers only.

### Windows — FB engine fully embedded in the MSI
`build/windows/scripts/embed_ohis.ps1` (`EmbedFluentBit`) downloads the whole engine at
build time, for two versions side by side:
- **FB 3.x (current)** → `logging\nrfb2\`:
  - `fluent-bit.exe` + `fluent-bit.dll` from
    `logging-fb-windows-packages.s3.us-east-2.amazonaws.com/fb-windows-5.0.6-<arch>.zip`
  - `out_newrelic.dll` from newrelic-fluent-bit-output GitHub release
- **FB 1.9 (legacy)** → `logging\nrfb\`:
  - `fluent-bit.exe` from `newrelic-experimental/fluent-bit-package` GitHub zip
  - legacy `out_newrelic.dll`
- Both `fluent-bit.exe` get code-signed (`SignExecutable`).

WiX installer `build/package/windows/.../Product.wxs` installs them into:
- `LoggingToolFB2` → `logging\`        (FB 3.x, from `nrfb2`)
- `LoggingTool`    → `logging-legacy\` (FB 1.9, from `nrfb`; wrapped in
  "To be removed on removal of the ff fluent_bit_19" comments)

Maps to `supervisor_fb_conf_windows.go`'s `logging` vs `logging-legacy` dirs, chosen at
runtime by the `fluent_bit_19_win` feature flag.

### Summary table

| | Linux | Windows |
|---|---|---|
| FB engine | From distro repo as pkg dependency → `/opt/fluent-bit/bin` | Embedded in MSI → agent's `newrelic-integrations\logging(-legacy)` |
| Engine version pin | Not pinned by agent (pkg mgr decides) | `fluent-bit.version`: FB 5.0.6 / legacy 1.9.3 |
| NR output plugin | `out_newrelic.so`, built via `fluent-bit.mk`, in agent pkg | `out_newrelic.dll`, via `embed_ohis.ps1`, in MSI |
| `parsers.conf` | In agent pkg | In MSI |
| Dependency strength | recommends (modern) / hard deps (AL2, CentOS 7, SLES 12.5) / none (CentOS 6) | N/A — self-contained |
| Version switch 1.9↔current | By binary on disk (`onlyTdAgentInstalled`) | By `fluent_bit_19_win` feature flag → dir choice |

---

## Key files reference
- `pkg/integrations/v4/supervisor_fb.go` — supervisor, executor command, availability.
- `pkg/integrations/v4/supervisor_fb_conf_{linux,windows,others}.go` — path/version per OS.
- `pkg/integrations/v4/logs/` — logging.d YAML → FB config (`LoadAndFormat`), watcher.
  - `loader.go` — `CfgLoader`: reads configs, resolves entity GUID + hostname, calls `NewFBConf` (`loader.go:71-78`).
  - `cfg.go` — `LogCfg` YAML schema; `NewFBConf` builds inputs/filters/output; metadata `record_modifier` injection (`cfg.go:280,334`).
  - `cfg_template.go` — `text/template` rendering `FBCfg` → `.conf`; `${NR_LICENSE_KEY_ENV_VAR}` in `[OUTPUT]`.
- `pkg/config/config.go` (+ `config_{linux,windows}.go`) — FB path defaults.
- `internal/agent/cmdchannel/fflag/ffhandler.go` — `fluent_bit_19_win` FF + FB restart.
- `cmd/newrelic-infra/newrelic-infra.go:~460` — supervisor wiring at startup.
- `build/embed/fluent-bit.{mk,version}` — Linux NR plugin download + version pins.
- `build/embed/integrations.mk` — sibling OHI embedding (nri-docker/flex/prometheus).
- `build/goreleaser/linux/*.yml` — Linux packaging + FB dependency declarations.
- `build/windows/scripts/embed_ohis.ps1` — Windows FB engine download/embed/sign.
- `build/package/windows/.../Product.wxs` — Windows MSI install layout.

---

## How the Fluent Bit `.deb` is dynamically linked (standalone-package findings)

Discovered while building the standalone on-host POC package (the e2e downloads the apt-pool
`fluent-bit_<ver>_<distro>_<arch>.deb`, extracts just the `fluent-bit` binary + `parsers.conf`,
adds `out_newrelic.so`, and runs it directly — *without* letting apt resolve the `.deb`'s
declared dependencies). Two hard truths surfaced that dictate how a self-contained package must
be assembled.

### The apt `.deb` is NOT self-contained

`dpkg-deb -x` of the `.deb` yields only:
```
./opt/fluent-bit/bin/fluent-bit          # the engine (≈71 MB, statically bundles FB's own code)
./lib/fluent-bit/libfluent-bit.so        # engine as a shared lib (not used when running the bin)
./etc/fluent-bit/{fluent-bit,parsers,plugins}.conf
./usr/lib/systemd/system/fluent-bit.service
```
The engine binary is dynamically linked against **system libraries that the `.deb` does NOT
ship** — it only *declares* them as package `Depends:` (`libpq5`, `libsasl2-2`, `libyaml-0-2`,
`libcurl4`, `libssl3`, …). A normal `apt install fluent-bit` pulls those in; a manual extract
does not. So the engine will not run from a bare extract.

### The full transitive closure is small — size is not the constraint

`ldd fluent-bit` resolves the **complete** transitive set (every dependency-of-a-dependency, not
just direct `NEEDED`). On Ubuntu it is ~40 shared objects — libcurl alone drags in
krb5/ldap/gnutls/brotli/nghttp2/etc. — but the **total on-disk size is only ~6.9 MB**. The 71 MB
engine binary dwarfs all of it. Bundling every non-glibc lib would therefore be cheap; size is
not what makes a self-contained package hard.

### The real constraint: glibc ABI version, not missing files

Running the **`ubuntu-noble` (24.04)** binary on an older host fails at load time with:
```
libc.so.6:      version `GLIBC_2.38' not found
libm.so.6:      version `GLIBC_2.38' not found
libstdc++.so.6: version `GLIBCXX_3.4.32' not found
```
i.e. the engine is compiled against the glibc/libstdc++ symbol versions of its build distro and
requires **that version or newer** on the host. This is a versioned-symbol mismatch, not an
absent file — the host's `libc`/`libm`/`libstdc++` exist but are too old. A dynamically-linked
process that hits this aborts before `main` and exits **127** (the same code as
"command not found"), which is what Agent Control's supervisor logs as
`Executable exited unsuccessfully … exit_code: Some(127)`.

Consequences for packaging:
- **You cannot fix a version mismatch by bundling.** Satisfying `GLIBC_2.38` would mean shipping
  `libc.so.6` + the matching `ld-linux` + `libm`/`libstdc++`, i.e. the glibc/loader family.
  Bundling those couples the package to one glibc and typically breaks more than it fixes — never
  bundle libc or the loader.
- **The engine `.deb` must match the host's distro/release.** The correct lever is to select the
  `.deb` built for the host's Ubuntu release (`jammy`/`focal`/`noble`/…), detected at build time
  from `/etc/os-release` `VERSION_CODENAME`, rather than hardcoding one. Then the binary links
  against the glibc the host actually has, and every glibc-family error disappears.

### The New Relic output plugin is unaffected

`ldd out_newrelic.so` shows only `libpthread.so.0`, `libc.so.6`, `libresolv.so.2`, `ld-linux` —
all ubiquitous, all present, and **zero version errors**. It is a Go plugin: it links only
baseline glibc symbols and carries its own runtime, so it is tolerant of older hosts and needs
nothing bundled.

### What actually has to be provided, once the distro matches

With a distro-matched `.deb`, the transitive closure is almost entirely libraries every Ubuntu
host already has. The lone genuine gap is **`libpq.so.5`** (PostgreSQL client), a *hard* `NEEDED`
of the engine because the pgsql plugin is linked in — even though a `dummy`→`newrelic` pipeline
never uses it, the loader still requires the file present. It is a small leaf lib whose own
dependencies (krb5, ldap, …) are already satisfied by the curl chain, so it can be provided
either by bundling just that one file (+ `LD_LIBRARY_PATH`, keeping the package self-contained
and avoiding the glibc family) or by `apt install libpq5`.

### Implications for a real production package

A production self-contained Fluent Bit OCI package (the ingredient this POC exists to scope)
must, per OS/arch:
1. Build/obtain the engine for a **baseline-enough glibc** (or ship per-distro variants), since
   the engine's glibc floor is set at compile time and cannot be papered over at runtime.
2. Include the engine's non-glibc `NEEDED` closure that isn't guaranteed on the target — at
   minimum `libpq.so.5` — and expose it via `LD_LIBRARY_PATH`/RUNPATH, **without** bundling
   `libc`/`libm`/`libstdc++`/`ld-linux`.
3. Add `out_newrelic.so` + `parsers.conf` (both relocatable/tolerant as shown above).

# Example of generated config

```yaml
logging.yml:
  logs:
    - name: dpkg.log
      file: /var/log/dpkg.log
      attributes:
        logtype: linux_dpkg
    - name: syslog
      file: /var/log/syslog
      attributes:
        logtype: linux_syslog
```

Generated by infra-agent
```yaml
[INPUT]
    Name tail
    Path /var/log/dpkg.log
    Buffer_Max_Size 128k
    Mem_Buf_Limit 16384k
    Skip_Long_Lines On
    Path_Key filePath
    Tag  dpkg.log
    DB   /var/lib/newrelic-agent-control/packages/nr-infra/stored_packages/infra-agent/oci_newrelic__infrastructure_agent_artifacts_1_77_1/logging/fb.db

[INPUT]
    Name tail
    Path /var/log/syslog
    Buffer_Max_Size 128k
    Mem_Buf_Limit 16384k
    Skip_Long_Lines On
    Path_Key filePath
    Tag  syslog
    DB   /var/lib/newrelic-agent-control/packages/nr-infra/stored_packages/infra-agent/oci_newrelic__infrastructure_agent_artifacts_1_77_1/logging/fb.db

[FILTER]
    Name  record_modifier
    Match dpkg.log
    Record "fb.input" "tail"
    Record "logtype" "linux_dpkg"

[FILTER]
    Name  record_modifier
    Match syslog
    Record "fb.input" "tail"
    Record "logtype" "linux_syslog"

[FILTER]
    Name  record_modifier
    Match *
    Record "entity.guid.INFRA" "NjIzNTM4MnxJTkZSQXxOQXwxNjE4MjEzNzcwMTc5Mzg5Njg"
    Record "hostname" "ac-vagrant-20260713114611"
    Record "plugin.type" "nri-agent"

[OUTPUT]
    Name                newrelic
    Match               *
    licenseKey          ${NR_LICENSE_KEY_ENV_VAR}
    validateProxyCerts  false
    Retry_Limit         5
```
