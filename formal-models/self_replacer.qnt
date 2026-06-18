// Tier-1 starter model — newrelic-agent-control formal-methods learning project.
//
// Models the binary self-replacement protocol implemented in
//   self-replacer/src/replacer.rs:92-164  (function `replace_binary`)
//
// Filesystem is abstracted as a total map  Path -> Content,
// where Content includes a sentinel `Absent` for "no file at this path".
// A `crashed` flag is sticky: once set, only stutter steps fire — modelling
// the OS supervisor (systemd / Kubernetes / Windows Service Manager) restarting
// the process from `CURRENT`. The central safety claim is that whenever a
// crash could happen, `CURRENT` still holds a runnable binary.
//
// HOW TO RUN
//   npm i -g @informalsystems/quint
//   quint test    formal-models/self_replacer.qnt
//   quint run     --invariant=safety --max-steps=20  formal-models/self_replacer.qnt
//   quint verify  --invariant=safety  formal-models/self_replacer.qnt
//
// EXPERIMENTS TO TRY (each one teaches something specific):
//   1. Run as-is. `safety` should hold and the two `run` tests should pass.
//   2. In `step_action`, comment out `create_backup_unix` and uncomment
//      `create_backup_windows`. Re-run with `--invariant=safety` and look
//      at the counterexample trace. (Hint: there is a window where neither
//      CURRENT nor TEMP holds a binary.)
//   3. Split `atomic_rename` into rename + fsync_dir, and let `crash` fire
//      between them. Does `safety` still hold? What about durability after
//      restart on a real filesystem with delayed metadata flush?
//   4. Model two concurrent self_replace calls (race two operators). What
//      new invariants do you need? Hint: the temp filename is a constant.

module self_replacer {

  // ---------------- types ----------------

  type Content = Absent | OldBin | NewBin
  type Path    = str
  type Step    = Init | BackupCreated | TempWritten | DoneOk | DoneRolledBack

  // ---------------- constants ----------------

  pure val CURRENT: Path = "/usr/bin/agent"
  pure val BACKUP:  Path = "/usr/bin/agent.bak"
  pure val TEMP:    Path = "/usr/bin/__temp__agent"

  // ---------------- state ----------------

  var fs:      Path -> Content
  var step:    Step
  var crashed: bool

  // ---------------- helpers ----------------

  pure def isBin(c: Content): bool = c == OldBin or c == NewBin

  // ---------------- init ----------------
  // Only CURRENT exists, holding the running binary.
  action init = all {
    fs'      = Map(CURRENT -> OldBin, BACKUP -> Absent, TEMP -> Absent),
    step'    = Init,
    crashed' = false,
  }

  // ---------------- happy-path actions ----------------
  // Each action models a single line/region of `replace_binary`.

  // replacer.rs:110 (Unix path)  fs::copy(current_exe, &backup)
  // The OS keeps the running binary's inode alive; CURRENT remains valid.
  action create_backup_unix = all {
    not(crashed),
    step == Init,
    fs'      = fs.put(BACKUP, fs.get(CURRENT)),
    step'    = BackupCreated,
    crashed' = crashed,
  }

  // replacer.rs:125 (Windows path)  fs::rename(current_exe, &backup)
  // Critical asymmetry: rename moves the file. Between this and the final
  // rename, CURRENT does NOT exist. Toggle this in `step_action` to break
  // `safety` and study the trace.
  action create_backup_windows = all {
    not(crashed),
    step == Init,
    fs'      = fs.put(CURRENT, Absent).put(BACKUP, OldBin),
    step'    = BackupCreated,
    crashed' = crashed,
  }

  // replacer.rs:127  create_temp_file(...)
  // Copies the new binary to a temp path in the same directory, so the
  // next rename is a same-FS rename (atomic on POSIX).
  action write_temp = all {
    not(crashed),
    step == BackupCreated,
    fs'      = fs.put(TEMP, NewBin),
    step'    = TempWritten,
    crashed' = crashed,
  }

  // replacer.rs:135  fs::rename(&new_bin_temp, current_exe)  — atomic on same FS
  action atomic_rename = all {
    not(crashed),
    step == TempWritten,
    fs'      = fs.put(CURRENT, NewBin).put(TEMP, Absent),
    step'    = DoneOk,
    crashed' = crashed,
  }

  // replacer.rs:147  fallback when the previous rename failed
  // The real code does `fs::rename(&backup, current_exe)` — modelled here as
  // "restore CURRENT from BACKUP, drop TEMP".
  action rollback = all {
    not(crashed),
    step == TempWritten,
    fs.get(BACKUP) != Absent,
    fs'      = fs.put(CURRENT, fs.get(BACKUP)).put(BACKUP, Absent).put(TEMP, Absent),
    step'    = DoneRolledBack,
    crashed' = crashed,
  }

  // ---------------- adversary ----------------

  // The OS can yank power at any moment. After this fires, `crashed` stays
  // true and only `stutter` fires — representing a halted process. The OS
  // supervisor then re-execs whatever is at CURRENT.
  action crash = all {
    not(crashed),
    fs'      = fs,
    step'    = step,
    crashed' = true,
  }

  action stutter = all {
    fs' = fs, step' = step, crashed' = crashed,
  }

  action step_action = any {
    create_backup_unix,
    // create_backup_windows,   // <- swap to study the Windows-rename window
    write_temp,
    atomic_rename,
    rollback,
    crash,
    stutter,
  }

  // ---------------- properties ----------------

  // SAFETY (durability): at every reachable state, CURRENT contains a
  // runnable binary. If the process crashes here, the OS supervisor can
  // safely re-exec it. This is the property the careful step ordering in
  // `replace_binary` is designed to maintain.
  val safety: bool = isBin(fs.get(CURRENT))

  // PROGRESS-MARKER COHERENCE: states match what's on disk in the unix
  // happy path. Useful as a sanity check while you extend the model.
  val coherent: bool =
    if (step == Init)            fs.get(BACKUP) == Absent and fs.get(TEMP) == Absent
    else if (step == DoneOk)     fs.get(CURRENT) == NewBin and fs.get(TEMP) == Absent
    else if (step == DoneRolledBack) fs.get(CURRENT) == OldBin and fs.get(TEMP) == Absent
    else true

  // ---------------- positive traces ----------------

  // Happy path: four lines fire in order, CURRENT becomes NewBin.
  run happy_path_test =
    init
      .then(create_backup_unix)
      .then(write_temp)
      .then(atomic_rename)
      .then(assert(fs.get(CURRENT) == NewBin and step == DoneOk))

  // Rollback path: rename fails after temp written, BACKUP restores CURRENT.
  run rollback_test =
    init
      .then(create_backup_unix)
      .then(write_temp)
      .then(rollback)
      .then(assert(fs.get(CURRENT) == OldBin and step == DoneRolledBack))

  // Crash before the final rename leaves the unix path with a valid CURRENT.
  run unix_crash_after_temp_test =
    init
      .then(create_backup_unix)
      .then(write_temp)
      .then(crash)
      .then(assert(isBin(fs.get(CURRENT))))
}
